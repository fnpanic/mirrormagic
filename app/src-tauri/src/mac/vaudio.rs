//! The PC's microphones and speakers as audio devices on the Mac, one per PC device ("Mikrofon
//! (Jabra Link 390) – PC"), without a driver (corporate Macs can't install BlackHole without admin
//! rights). Built from Core Audio taps (macOS 14.2+):
//! - PC input: a public aggregate device whose input is a muted tap on MirrorMagic itself.
//!   MirrorMagic plays that PC microphone to the default output; the tap mutes it there and hands
//!   it to whoever records from the device (Teams, Zoom, a browser).
//! - PC output: a public aggregate device around the real speakers. A muted tap on its output
//!   (all processes but MirrorMagic) silences it on the Mac; MirrorMagic reads that tap through a
//!   private aggregate ("MirrorMagic tap: …") and the PC plays it on that output.
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

/// An app (Teams…) records from or plays to this device right now. MirrorMagic itself never runs
/// I/O on these public devices (it reads the speaker taps through private ones), so the device's
/// own "running somewhere" flag answers it. (0.5.12–0.5.14 asked each process for its devices
/// instead; that list doesn't name aggregate devices, so use was never noticed.)
pub fn in_use(dev: u32) -> bool {
    get::<u32>(dev, cc(b"gone"), None).unwrap_or(0) != 0
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

/// The devices while they exist; destroyed on drop (coreaudiod would keep them, with a dead tap).
pub struct Devices {
    /// (device, PC input name)
    pub mics: Vec<(u32, String)>,
    /// (device, PC output name)
    pub speakers: Vec<(u32, String)>,
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

const UID: &str = "app.mirrormagic.";

/// Public aggregate devices outlive the process that made them (coreaudiod keeps them), with a
/// dead tap inside: remove every one of ours an earlier run left.
fn destroy_stale(log: &mut Vec<String>) {
    for id in list(SYSTEM, cc(b"dev#"), GLOBAL) {
        if let Some(uid) = device_uid(id).filter(|u| u.starts_with(UID)) {
            let e = unsafe { AudioHardwareDestroyAggregateDevice(id) };
            log.push(format!("removed stale {uid} (device {id}): {e}"));
        }
    }
}

/// What a PC device is called on the Mac (Teams adds "(Aggregate)").
fn mac_name(pc: &str) -> String {
    format!("{pc} – PC")
}

/// The private device MirrorMagic reads a PC output's sound from.
pub fn reader_name(pc: &str) -> String {
    format!("MirrorMagic tap: {pc}")
}

/// One Mac device per PC input and output; `me` is this process's audio object. Each device
/// fails on its own.
pub fn create(me: u32, inputs: &[String], outputs: &[String]) -> (Devices, Vec<String>) {
    let mut d = Devices { mics: vec![], speakers: vec![], taps: vec![], devs: vec![] };
    let mut log = vec![];
    destroy_stale(&mut log);
    // the real output to play through: the default output, unless it is ours
    let out = get::<u32>(SYSTEM, cc(b"dOut"), None).and_then(device_uid).filter(|u| !u.starts_with(UID));

    // PC input: a muted tap on this process, which plays that PC microphone while it is used
    for pc in inputs {
        let r = tap(false, &[me], None, &format!("MirrorMagic {pc}")).and_then(|(t, tuid)| {
            d.taps.push(t);
            let uid = format!("{UID}in.{pc}");
            aggregate(&mac_name(pc), &uid, false, None, Some(&tuid)).or_else(|_| aggregate(&mac_name(pc), &format!("{uid}.2"), false, out.as_deref(), Some(&tuid)))
        });
        match r {
            Ok(id) => {
                d.devs.push(id);
                d.mics.push((id, pc.clone()));
            }
            Err(e) => log.push(format!("{pc}: {e}")),
        }
    }

    // PC output: aggregate around the real output, its output tapped (muted) for the PC
    for pc in outputs {
        let r = out.clone().ok_or_else(|| "no default output".to_string()).and_then(|out| {
            let uid = format!("{UID}out.{pc}");
            let dev = aggregate(&mac_name(pc), &uid, false, Some(&out), None)?;
            d.devs.push(dev);
            let (t, tuid) = tap(true, &[me], Some(&uid), &format!("MirrorMagic {pc}"))?;
            d.taps.push(t);
            let tap_uid = format!("{UID}tap.{pc}");
            let reader = aggregate(&reader_name(pc), &tap_uid, true, None, Some(&tuid)).or_else(|_| aggregate(&reader_name(pc), &format!("{tap_uid}.2"), true, Some(&out), Some(&tuid)))?;
            d.devs.push(reader);
            Ok(dev)
        });
        match r {
            Ok(id) => d.speakers.push((id, pc.clone())),
            Err(e) => log.push(format!("{pc}: {e}")),
        }
    }
    log.push(format!("{} microphone(s), {} speaker(s) of the PC offered", d.mics.len(), d.speakers.len()));
    (d, log)
}
