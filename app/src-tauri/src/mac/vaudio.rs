//! "PC Microphone" and "PC Speakers" as audio devices on the Mac, without a driver (corporate
//! Macs can't install BlackHole without admin rights). Built from Core Audio taps (macOS 14.2+):
//! - PC Microphone: a public aggregate device whose input is a muted tap on MirrorMagic itself.
//!   MirrorMagic plays the PC microphone to the default output; the tap mutes it there and hands
//!   it to whoever records from "PC Microphone" (Teams, Zoom, a browser).
//! - PC Speakers: a public aggregate device around the real speakers. A muted tap on its output
//!   (all processes but MirrorMagic) silences it on the Mac; MirrorMagic reads that tap through a
//!   private aggregate ("MirrorMagic speaker tap") and sends the sound to the PC.
//! The first tap read makes macOS ask for "System Audio Recording" permission.
use core_foundation::array::CFArray;
use core_foundation::base::{CFType, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::string::{CFString, CFStringRef};
use objc2::AllocAnyThread;
use objc2_core_audio::{CATapDescription, CATapMuteBehavior};
use objc2_foundation::{NSArray, NSNumber, NSString};
use std::ffi::c_void;

pub const MIC_NAME: &str = "PC Microphone";
pub const SPEAKER_NAME: &str = "PC Speakers";
/// Private aggregate that only MirrorMagic sees; cpal opens it by this name.
pub const SPEAKER_TAP_NAME: &str = "MirrorMagic speaker tap";

#[repr(C)]
struct Addr {
    selector: u32,
    scope: u32,
    element: u32,
}
const fn cc(s: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*s)
}
const SYSTEM: u32 = 1;
const GLOBAL: u32 = cc(b"glob");

#[link(name = "CoreAudio", kind = "framework")]
extern "C" {
    fn AudioObjectGetPropertyDataSize(obj: u32, addr: *const Addr, qsize: u32, q: *const c_void, size: *mut u32) -> i32;
    fn AudioObjectGetPropertyData(obj: u32, addr: *const Addr, qsize: u32, q: *const c_void, size: *mut u32, data: *mut c_void) -> i32;
    fn AudioHardwareCreateProcessTap(desc: *const CATapDescription, out: *mut u32) -> i32;
    fn AudioHardwareDestroyProcessTap(tap: u32) -> i32;
    fn AudioHardwareCreateAggregateDevice(desc: CFDictionaryRef, out: *mut u32) -> i32;
    fn AudioHardwareDestroyAggregateDevice(dev: u32) -> i32;
}

fn get<T: Default>(obj: u32, selector: u32, q: Option<&u32>) -> Option<T> {
    let mut v = T::default();
    let mut size = std::mem::size_of::<T>() as u32;
    let (qs, qp) = q.map_or((0, std::ptr::null()), |q| (4, q as *const u32 as *const c_void));
    let a = Addr { selector, scope: GLOBAL, element: 0 };
    (unsafe { AudioObjectGetPropertyData(obj, &a, qs, qp, &mut size, &mut v as *mut T as *mut c_void) } == 0).then_some(v)
}

fn device_uid(dev: u32) -> Option<String> {
    let s: CFStringRef = get::<usize>(dev, cc(b"uid "), None)? as CFStringRef;
    (!s.is_null()).then(|| unsafe { CFString::wrap_under_create_rule(s) }.to_string())
}

/// This process as a Core Audio process object (0 until it has talked to the audio system).
pub fn own_process_object() -> u32 {
    let pid = std::process::id();
    get::<u32>(SYSTEM, cc(b"id2p"), Some(&pid)).unwrap_or(0)
}

fn list(obj: u32, selector: u32, scope: u32) -> Vec<u32> {
    let a = Addr { selector, scope, element: 0 };
    let mut size = 0u32;
    if unsafe { AudioObjectGetPropertyDataSize(obj, &a, 0, std::ptr::null(), &mut size) } != 0 {
        return vec![];
    }
    let mut v = vec![0u32; size as usize / 4];
    if unsafe { AudioObjectGetPropertyData(obj, &a, 0, std::ptr::null(), &mut size, v.as_mut_ptr() as *mut c_void) } != 0 {
        return vec![];
    }
    v.truncate(size as usize / 4);
    v
}

/// Another process (Teams…) records from (`input`) or plays to this device right now. Asked per
/// process: MirrorMagic's own reading of the speaker tap must not count.
pub fn used_by_others(dev: u32, input: bool, me: u32) -> bool {
    let (running, scope) = if input { (cc(b"piri"), cc(b"inpt")) } else { (cc(b"piro"), cc(b"outp")) };
    list(SYSTEM, cc(b"prs#"), GLOBAL)
        .into_iter()
        .filter(|&p| p != me && get::<u32>(p, running, None).unwrap_or(0) != 0)
        .any(|p| list(p, cc(b"pdv#"), scope).contains(&dev))
}

/// A tap; returns (id, uid). `device` set: taps that device's first output stream.
fn tap(exclude: bool, procs: &[u32], device: Option<&str>, name: &str) -> Result<(u32, String), String> {
    let nums: Vec<_> = procs.iter().map(|&p| NSNumber::new_u32(p)).collect();
    let arr = NSArray::from_retained_slice(&nums);
    let d = unsafe {
        match (device, exclude) {
            (Some(uid), true) => CATapDescription::initExcludingProcesses_andDeviceUID_withStream(CATapDescription::alloc(), &arr, &NSString::from_str(uid), 0),
            (Some(uid), false) => CATapDescription::initWithProcesses_andDeviceUID_withStream(CATapDescription::alloc(), &arr, &NSString::from_str(uid), 0),
            (None, _) => CATapDescription::initStereoMixdownOfProcesses(CATapDescription::alloc(), &arr),
        }
    };
    unsafe {
        d.setName(&NSString::from_str(name));
        d.setPrivate(false);
        d.setMuteBehavior(CATapMuteBehavior::Muted);
    }
    let mut id = 0u32;
    let e = unsafe { AudioHardwareCreateProcessTap(&*d, &mut id) };
    if e != 0 || id == 0 {
        return Err(format!("tap {name}: error {e}"));
    }
    let uid = unsafe { d.UUID().UUIDString() }.to_string();
    Ok((id, uid))
}

fn aggregate(name: &str, uid: &str, private: bool, sub: Option<&str>, tap: Option<&str>) -> Result<u32, String> {
    let s = |x: &str| CFString::new(x).as_CFType();
    let mut pairs: Vec<(CFString, CFType)> = vec![
        (CFString::new("name"), s(name)),
        (CFString::new("uid"), s(uid)),
        (CFString::new("private"), CFBoolean::from(private).as_CFType()),
        (CFString::new("stacked"), CFBoolean::false_value().as_CFType()),
        (CFString::new("tapautostart"), CFBoolean::true_value().as_CFType()),
    ];
    if let Some(sub) = sub {
        let d = CFDictionary::from_CFType_pairs(&[(CFString::new("uid"), s(sub))]);
        pairs.push((CFString::new("subdevices"), CFArray::from_CFTypes(&[d]).as_CFType()));
        pairs.push((CFString::new("master"), s(sub)));
    }
    if let Some(t) = tap {
        let d = CFDictionary::from_CFType_pairs(&[(CFString::new("uid"), s(t)), (CFString::new("drift"), CFBoolean::true_value().as_CFType())]);
        pairs.push((CFString::new("taps"), CFArray::from_CFTypes(&[d]).as_CFType()));
    }
    let dict = CFDictionary::from_CFType_pairs(&pairs);
    let mut id = 0u32;
    let e = unsafe { AudioHardwareCreateAggregateDevice(dict.as_concrete_TypeRef(), &mut id) };
    if e != 0 || id == 0 {
        return Err(format!("device {name}: error {e}"));
    }
    Ok(id)
}

/// The devices while they exist; destroyed on drop (macOS also removes them when the app ends).
pub struct Devices {
    pub mic: Option<u32>,
    pub speaker: Option<u32>,
    taps: Vec<u32>,
    devs: Vec<u32>,
}

impl Drop for Devices {
    fn drop(&mut self) {
        for &d in &self.devs {
            unsafe { AudioHardwareDestroyAggregateDevice(d) };
        }
        for &t in &self.taps {
            unsafe { AudioHardwareDestroyProcessTap(t) };
        }
    }
}

const UIDS: [&str; 5] = ["app.mirrormagic.mic", "app.mirrormagic.mic2", "app.mirrormagic.speakers", "app.mirrormagic.spktap", "app.mirrormagic.spktap2"];

/// Public aggregate devices outlive the process that made them (coreaudiod keeps them), with a
/// dead tap inside: remove the ones an earlier run left.
fn destroy_stale(log: &mut Vec<String>) {
    for uid in UIDS {
        let q = CFString::new(uid);
        let r = q.as_concrete_TypeRef();
        let a = Addr { selector: cc(b"uidd"), scope: GLOBAL, element: 0 };
        let (mut id, mut size) = (0u32, 4u32);
        let qp = &r as *const CFStringRef as *const c_void;
        let ok = unsafe { AudioObjectGetPropertyData(SYSTEM, &a, std::mem::size_of::<CFStringRef>() as u32, qp, &mut size, &mut id as *mut u32 as *mut c_void) } == 0;
        if ok && id != 0 {
            let e = unsafe { AudioHardwareDestroyAggregateDevice(id) };
            log.push(format!("removed stale {uid} (device {id}): {e}"));
        }
    }
}

/// Creates both devices; `me` is this process's audio object. Each half fails on its own.
pub fn create(me: u32) -> (Devices, Vec<String>) {
    let mut d = Devices { mic: None, speaker: None, taps: vec![], devs: vec![] };
    let mut log = vec![];
    destroy_stale(&mut log);
    // the real output to play through: the default output at start, unless it is ours
    let out = get::<u32>(SYSTEM, cc(b"dOut"), None).and_then(device_uid).filter(|u| !u.starts_with("app.mirrormagic"));

    // PC Microphone: tap on this process (muted), no output of its own
    let mic = tap(false, &[me], None, "MirrorMagic PC microphone").and_then(|(t, tuid)| {
        d.taps.push(t);
        aggregate(MIC_NAME, "app.mirrormagic.mic", false, None, Some(&tuid))
            .or_else(|e| {
                log.push(format!("{e}, retrying with the speakers as clock"));
                aggregate(MIC_NAME, "app.mirrormagic.mic2", false, out.as_deref(), Some(&tuid))
            })
    });
    match mic {
        Ok(id) => {
            d.devs.push(id);
            d.mic = Some(id);
            log.push(format!("{MIC_NAME}: device {id}"));
        }
        Err(e) => log.push(format!("{MIC_NAME} failed: {e}")),
    }

    // PC Speakers: aggregate around the real output, its output tapped (muted) for the PC
    let spk = out.ok_or_else(|| "no default output".to_string()).and_then(|out| {
        let uid = "app.mirrormagic.speakers";
        let dev = aggregate(SPEAKER_NAME, uid, false, Some(&out), None)?;
        d.devs.push(dev);
        let (t, tuid) = tap(true, &[me], Some(uid), "MirrorMagic PC speakers")?;
        d.taps.push(t);
        let reader = aggregate(SPEAKER_TAP_NAME, "app.mirrormagic.spktap", true, None, Some(&tuid))
            .or_else(|_| aggregate(SPEAKER_TAP_NAME, "app.mirrormagic.spktap2", true, Some(&out), Some(&tuid)))?;
        d.devs.push(reader);
        Ok(dev)
    });
    match spk {
        Ok(id) => {
            d.speaker = Some(id);
            log.push(format!("{SPEAKER_NAME}: device {id}"));
        }
        Err(e) => log.push(format!("{SPEAKER_NAME} failed: {e}")),
    }
    (d, log)
}
