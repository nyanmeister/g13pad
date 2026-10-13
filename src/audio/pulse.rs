// SPDX-License-Identifier: GPL-3.0-or-later
//! Native PulseAudio protocol client (also PipeWire-Pulse). One owning thread;
//! callbacks borrow stable boxed state only during mainloop iteration. No subprocesses.
use super::Action;
use std::{
    ffi::{c_char, c_int, c_void, CStr, CString},
    ptr, thread,
    time::{Duration, Instant},
};

#[repr(C)]
#[derive(Clone, Copy)]
struct SampleSpec {
    format: c_int,
    rate: u32,
    channels: u8,
}
#[repr(C)]
struct ChannelMap {
    channels: u8,
    map: [c_int; 32],
}
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct Volume {
    channels: u8,
    values: [u32; 32],
}
// Only the documented common prefix is read, never passed by value to C.
#[repr(C)]
struct DeviceInfo {
    name: *const c_char,
    index: u32,
    description: *const c_char,
    sample: SampleSpec,
    map: ChannelMap,
    owner: u32,
    volume: Volume,
    mute: c_int,
    monitor: u32,
}
#[repr(C)]
struct ServerInfo {
    user: *const c_char,
    host: *const c_char,
    version: *const c_char,
    name: *const c_char,
    sample: SampleSpec,
    sink: *const c_char,
    source: *const c_char,
}
#[repr(C)]
struct BufferAttr {
    maxlength: u32,
    tlength: u32,
    prebuf: u32,
    minreq: u32,
    fragsize: u32,
}
type Handle = *mut c_void;
type InfoCallback = unsafe extern "C" fn(Handle, *const DeviceInfo, c_int, Handle);
type SuccessCallback = unsafe extern "C" fn(Handle, c_int, Handle);

#[link(name = "pulse")]
unsafe extern "C" {
    fn pa_mainloop_new() -> Handle;
    fn pa_mainloop_free(m: Handle);
    fn pa_mainloop_get_api(m: Handle) -> Handle;
    fn pa_mainloop_iterate(m: Handle, block: c_int, result: *mut c_int) -> c_int;
    fn pa_context_new(api: Handle, name: *const c_char) -> Handle;
    fn pa_context_connect(c: Handle, server: *const c_char, flags: u32, api: Handle) -> c_int;
    fn pa_context_get_state(c: Handle) -> c_int;
    fn pa_context_errno(c: Handle) -> c_int;
    fn pa_strerror(error: c_int) -> *const c_char;
    fn pa_context_disconnect(c: Handle);
    fn pa_context_unref(c: Handle);
    fn pa_operation_cancel(o: Handle);
    fn pa_operation_unref(o: Handle);
    fn pa_context_get_server_info(
        c: Handle,
        cb: unsafe extern "C" fn(Handle, *const ServerInfo, Handle),
        data: Handle,
    ) -> Handle;
    fn pa_context_get_source_info_by_name(
        c: Handle,
        name: *const c_char,
        cb: InfoCallback,
        data: Handle,
    ) -> Handle;
    fn pa_context_get_source_info_list(c: Handle, cb: InfoCallback, data: Handle) -> Handle;
    fn pa_context_get_sink_info_by_name(
        c: Handle,
        name: *const c_char,
        cb: InfoCallback,
        data: Handle,
    ) -> Handle;
    fn pa_context_set_subscribe_callback(
        c: Handle,
        cb: unsafe extern "C" fn(Handle, u32, u32, Handle),
        data: Handle,
    );
    fn pa_context_subscribe(
        c: Handle,
        mask: u32,
        cb: Option<SuccessCallback>,
        data: Handle,
    ) -> Handle;
    fn pa_context_set_source_mute_by_index(
        c: Handle,
        index: u32,
        mute: c_int,
        cb: SuccessCallback,
        data: Handle,
    ) -> Handle;
    fn pa_context_set_sink_mute_by_index(
        c: Handle,
        index: u32,
        mute: c_int,
        cb: SuccessCallback,
        data: Handle,
    ) -> Handle;
    fn pa_context_set_source_volume_by_index(
        c: Handle,
        index: u32,
        volume: *const Volume,
        cb: SuccessCallback,
        data: Handle,
    ) -> Handle;
    fn pa_context_set_sink_volume_by_index(
        c: Handle,
        index: u32,
        volume: *const Volume,
        cb: SuccessCallback,
        data: Handle,
    ) -> Handle;
    fn pa_cvolume_scale(v: *mut Volume, max: u32) -> *mut Volume;
    fn pa_stream_new(
        c: Handle,
        name: *const c_char,
        spec: *const SampleSpec,
        map: Handle,
    ) -> Handle;
    fn pa_stream_set_read_callback(
        s: Handle,
        cb: unsafe extern "C" fn(Handle, usize, Handle),
        data: Handle,
    );
    fn pa_stream_connect_record(
        s: Handle,
        device: *const c_char,
        attr: *const BufferAttr,
        flags: u32,
    ) -> c_int;
    fn pa_stream_get_state(s: Handle) -> c_int;
    fn pa_stream_peek(s: Handle, data: *mut *const c_void, bytes: *mut usize) -> c_int;
    fn pa_stream_drop(s: Handle) -> c_int;
    fn pa_stream_disconnect(s: Handle) -> c_int;
    fn pa_stream_unref(s: Handle);
}

#[derive(Clone)]
pub(super) struct Device {
    pub name: String,
    pub description: String,
    pub index: u32,
    pub muted: bool,
    volume: Volume,
}
impl Device {
    pub fn percent(&self) -> u32 {
        ((self.volume.maximum() as u64 * 100 + 32768) / 65536) as u32
    }
}
impl Volume {
    fn maximum(&self) -> u32 {
        self.values[..usize::from(self.channels).min(32)]
            .iter()
            .copied()
            .max()
            .unwrap_or(0)
    }
    fn adjusted(mut self, up: bool) -> Self {
        let current = self.maximum();
        // Up never lowers an existing boosted level; normal raises stop at100%.
        let target = if up {
            current.max(current.saturating_add(3277).min(65536))
        } else {
            current.saturating_sub(3277)
        };
        unsafe {
            pa_cvolume_scale(&mut self, target);
        }
        self
    }
}
#[derive(Default)]
struct State {
    microphone: Option<CString>,
    inputs: Vec<super::Input>,
    inputs_done: bool,
    inputs_failed: bool,
    source: Option<Device>,
    sink: Option<Device>,
    dirty: bool,
    pending_source: Option<Device>,
    pending_sink: Option<Device>,
    source_done: bool,
    sink_done: bool,
    querying: bool,
    ack: Option<bool>,
    peak: Option<(f32, Instant)>,
}
unsafe fn string(p: *const c_char) -> String {
    if p.is_null() {
        String::new()
    } else {
        CStr::from_ptr(p).to_string_lossy().into_owned()
    }
}
unsafe fn device(i: *const DeviceInfo) -> Option<Device> {
    let i = i.as_ref()?;
    if i.volume.channels == 0 || i.volume.channels > 32 {
        return None;
    }
    Some(Device {
        name: string(i.name),
        description: string(i.description),
        index: i.index,
        muted: i.mute != 0,
        volume: i.volume,
    })
}
unsafe extern "C" fn source_info(_: Handle, i: *const DeviceInfo, eol: c_int, data: Handle) {
    let state = &mut *(data as *mut State);
    if eol != 0 {
        state.source_done = true;
    } else {
        state.pending_source = device(i);
    }
}
unsafe extern "C" fn sink_info(_: Handle, i: *const DeviceInfo, eol: c_int, data: Handle) {
    let state = &mut *(data as *mut State);
    if eol != 0 {
        state.sink_done = true;
    } else {
        state.pending_sink = device(i);
    }
}
unsafe extern "C" fn input_info(_: Handle, i: *const DeviceInfo, eol: c_int, data: Handle) {
    let state = &mut *(data as *mut State);
    if eol != 0 {
        state.inputs_done = true;
        state.inputs_failed = eol < 0;
    } else if let Some(i) = i.as_ref() {
        state.inputs.push(super::Input {
            name: string(i.name),
            description: string(i.description),
            monitor: i.monitor != u32::MAX,
        });
    }
}
unsafe fn forget(o: Handle) {
    if !o.is_null() {
        pa_operation_unref(o);
    }
}
unsafe extern "C" fn server_info(c: Handle, info: *const ServerInfo, data: Handle) {
    let state = &mut *(data as *mut State);
    let Some(info) = info.as_ref() else {
        state.source_done = true;
        state.sink_done = true;
        return;
    };
    let source = state
        .microphone
        .as_ref()
        .map_or(info.source, |s| s.as_ptr());
    if !source.is_null() {
        let o = pa_context_get_source_info_by_name(c, source, source_info, data);
        state.source_done = o.is_null();
        forget(o);
    } else {
        state.source_done = true;
    }
    if !info.sink.is_null() {
        let o = pa_context_get_sink_info_by_name(c, info.sink, sink_info, data);
        state.sink_done = o.is_null();
        forget(o);
    } else {
        state.sink_done = true;
    }
}
unsafe extern "C" fn subscribed(_: Handle, event: u32, _: u32, data: Handle) {
    if matches!(event & 15, 0 | 1 | 7) {
        (*(data as *mut State)).dirty = true;
    }
}
unsafe extern "C" fn success(_: Handle, ok: c_int, data: Handle) {
    (*(data as *mut State)).ack = Some(ok != 0);
}
unsafe extern "C" fn peaks(s: Handle, _: usize, data: Handle) {
    let state = &mut *(data as *mut State);
    let mut raw = ptr::null();
    let mut bytes = 0;
    if pa_stream_peek(s, &mut raw, &mut bytes) < 0 || bytes == 0 {
        return;
    }
    let mut peak = 0f32;
    if !raw.is_null() && bytes <= 65536 && bytes % 4 == 0 {
        // Read bytes rather than assuming libpulse's pointer is float-aligned.
        for sample in std::slice::from_raw_parts(raw.cast::<u8>(), bytes)
            .as_chunks::<4>()
            .0
        {
            let v = f32::from_le_bytes([sample[0], sample[1], sample[2], sample[3]]);
            if v.is_finite() {
                peak = peak.max(v.abs().min(1.0));
            }
        }
    }
    state.peak = Some((peak, Instant::now()));
    pa_stream_drop(s);
}

pub(super) struct Client {
    mainloop: Handle,
    context: Handle,
    stream: Handle,
    state: Box<State>,
    stream_source: String,
    refresh: Instant,
    stream_retry: Instant,
}
impl Client {
    pub fn connect(microphone: Option<&str>) -> Result<Self, String> {
        Self::connect_inner(microphone, Duration::from_secs(2), true)
    }
    fn connect_inner(
        microphone: Option<&str>,
        timeout: Duration,
        defaults: bool,
    ) -> Result<Self, String> {
        let microphone = microphone
            .map(|name| CString::new(name).map_err(|_| "invalid microphone identity"))
            .transpose()?;
        unsafe {
            let mainloop = pa_mainloop_new();
            if mainloop.is_null() {
                return Err("cannot allocate audio mainloop".into());
            }
            let context = pa_context_new(
                pa_mainloop_get_api(mainloop),
                c"g13pad system audio".as_ptr(),
            );
            let mut client = Self {
                mainloop,
                context,
                stream: ptr::null_mut(),
                state: Box::new(State {
                    microphone,
                    ..State::default()
                }),
                stream_source: String::new(),
                refresh: Instant::now(),
                stream_retry: Instant::now(),
            };
            if context.is_null() || pa_context_connect(context, ptr::null(), 1, ptr::null_mut()) < 0
            {
                return Err("cannot connect to the desktop audio server".into());
            }
            let deadline = Instant::now() + timeout;
            while pa_context_get_state(context) != 4 {
                client.iterate()?;
                if Instant::now() >= deadline {
                    return Err("audio server connection timed out".into());
                }
                thread::sleep(Duration::from_millis(5));
            }
            let data = client.data();
            pa_context_set_subscribe_callback(context, subscribed, data);
            forget(pa_context_subscribe(context, 0x83, None, ptr::null_mut()));
            if defaults {
                client.query();
                client.wait_query()?;
            }
            Ok(client)
        }
    }
    fn data(&mut self) -> Handle {
        (&mut *self.state as *mut State).cast()
    }
    fn error(&self) -> String {
        unsafe {
            format!(
                "audio server: {}",
                string(pa_strerror(pa_context_errno(self.context)))
            )
        }
    }
    fn iterate(&mut self) -> Result<(), String> {
        unsafe {
            // One iteration can dispatch only part of the ready protocol traffic.
            // Drain a bounded batch so peak packets cannot starve device replies
            // between the worker's 20ms sleeps. Stop immediately when idle.
            for _ in 0..32 {
                let dispatched = pa_mainloop_iterate(self.mainloop, 0, ptr::null_mut());
                if dispatched < 0 || pa_context_get_state(self.context) >= 5 {
                    return Err(self.error());
                }
                if dispatched == 0 {
                    break;
                }
            }
        }
        Ok(())
    }
    fn query(&mut self) {
        self.state.pending_source = None;
        self.state.pending_sink = None;
        self.state.source_done = false;
        self.state.sink_done = false;
        self.state.querying = true;
        self.state.dirty = false;
        self.refresh = Instant::now();
        unsafe {
            let data = self.data();
            let op = pa_context_get_server_info(self.context, server_info, data);
            if op.is_null() {
                self.state.source_done = true;
                self.state.sink_done = true;
            }
            forget(op);
        }
    }
    fn wait_query(&mut self) -> Result<(), String> {
        let deadline = Instant::now() + Duration::from_secs(2);
        while !(self.state.source_done && self.state.sink_done) {
            self.iterate()?;
            if Instant::now() >= deadline {
                return Err("audio device query timed out".into());
            }
            thread::sleep(Duration::from_millis(5));
        }
        self.finish_query();
        Ok(())
    }
    fn finish_query(&mut self) {
        self.state.source = self.state.pending_source.take();
        self.state.sink = self.state.pending_sink.take();
        self.state.querying = false;
    }
    pub fn devices(&self) -> (Option<Device>, Option<Device>) {
        (self.state.source.clone(), self.state.sink.clone())
    }
    pub fn inputs(&mut self) -> Result<super::Inputs, String> {
        self.state.inputs.clear();
        self.state.inputs_done = false;
        self.state.inputs_failed = false;
        let op = unsafe { pa_context_get_source_info_list(self.context, input_info, self.data()) };
        if op.is_null() {
            return Err(self.error());
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        let result = loop {
            if let Err(e) = self.iterate() {
                break Err(e);
            }
            if self.state.inputs_done {
                break if self.state.inputs_failed {
                    Err(self.error())
                } else {
                    Ok(())
                };
            }
            if Instant::now() >= deadline {
                break Err("microphone list timed out".into());
            }
            thread::sleep(Duration::from_millis(5));
        };
        unsafe {
            if result.is_err() {
                pa_operation_cancel(op);
            }
            pa_operation_unref(op);
        }
        result?;
        self.state.inputs.sort_by(|a, b| {
            a.monitor
                .cmp(&b.monitor)
                .then_with(|| a.description.cmp(&b.description))
                .then_with(|| a.name.cmp(&b.name))
        });
        Ok(super::Inputs {
            devices: std::mem::take(&mut self.state.inputs),
            default: self.state.source.as_ref().map(|d| d.name.clone()),
        })
    }
    pub fn peak(&self) -> Option<f32> {
        self.state
            .peak
            .filter(|(_, stamp)| stamp.elapsed() < Duration::from_millis(500))
            .map(|(v, _)| v)
    }
    fn stop_stream(&mut self) {
        unsafe {
            if !self.stream.is_null() {
                pa_stream_disconnect(self.stream);
                pa_stream_unref(self.stream);
                self.stream = ptr::null_mut();
            }
        }
        self.stream_source.clear();
        self.state.peak = None;
    }
    pub fn tick(&mut self, meter: bool) -> Result<(), String> {
        self.iterate()?;
        if self.state.querying {
            if self.state.source_done && self.state.sink_done {
                self.finish_query();
            } else if self.refresh.elapsed() > Duration::from_secs(2) {
                return Err("audio device query timed out".into());
            }
        }
        if !self.state.querying
            && (self.state.dirty || self.refresh.elapsed() >= Duration::from_secs(2))
        {
            self.query();
        }
        if self.state.querying {
            return Ok(());
        }
        let name = self
            .state
            .source
            .as_ref()
            .map(|d| d.name.clone())
            .unwrap_or_default();
        if !meter || (!self.stream.is_null() && name != self.stream_source) {
            self.stop_stream();
            self.stream_retry = Instant::now();
        }
        if meter && !name.is_empty() && self.stream.is_null() && Instant::now() >= self.stream_retry
        {
            unsafe {
                let spec = SampleSpec {
                    format: 5,
                    rate: 25,
                    channels: 1,
                };
                self.stream = pa_stream_new(
                    self.context,
                    c"g13pad microphone peaks".as_ptr(),
                    &spec,
                    ptr::null_mut(),
                );
                if self.stream.is_null() {
                    self.stream_retry = Instant::now() + Duration::from_secs(5);
                    return Ok(());
                }
                let data = self.data();
                pa_stream_set_read_callback(self.stream, peaks, data);
                let name_c =
                    CString::new(name.as_bytes()).map_err(|_| "invalid audio device name")?;
                let attr = BufferAttr {
                    maxlength: u32::MAX,
                    tlength: u32::MAX,
                    prebuf: u32::MAX,
                    minreq: u32::MAX,
                    fragsize: 4,
                };
                // Server-side peak detection, fixed device,40ms packets. No PCM history.
                if pa_stream_connect_record(
                    self.stream,
                    name_c.as_ptr(),
                    &attr,
                    0x0800 | 0x0200 | 0x2000,
                ) < 0
                {
                    self.stop_stream();
                    self.stream_retry = Instant::now() + Duration::from_secs(5);
                    return Ok(());
                }
                self.stream_source = name;
            }
        }
        unsafe {
            if !self.stream.is_null() && pa_stream_get_state(self.stream) >= 3 {
                self.stop_stream();
                self.stream_retry = Instant::now() + Duration::from_secs(5);
            }
        }
        Ok(())
    }
    pub fn perform(&mut self, action: Action) -> Result<(), String> {
        // Fresh observed state for every command, including two rapid toggle presses.
        if self.state.querying {
            self.wait_query()?;
        }
        self.query();
        self.wait_query()?;
        let microphone = action.microphone();
        let device = if microphone {
            &self.state.source
        } else {
            &self.state.sink
        };
        let device = device.clone().ok_or(if microphone {
            if self.state.microphone.is_some() {
                "selected microphone unavailable"
            } else {
                "default microphone unavailable"
            }
        } else {
            "default output unavailable"
        })?;
        self.state.ack = None;
        let data = self.data();
        let op = unsafe {
            match action {
                Action::MicToggle | Action::MicMute | Action::MicUnmute | Action::OutputToggle => {
                    let mute = match action {
                        Action::MicMute => true,
                        Action::MicUnmute => false,
                        _ => !device.muted,
                    } as c_int;
                    if microphone {
                        pa_context_set_source_mute_by_index(
                            self.context,
                            device.index,
                            mute,
                            success,
                            data,
                        )
                    } else {
                        pa_context_set_sink_mute_by_index(
                            self.context,
                            device.index,
                            mute,
                            success,
                            data,
                        )
                    }
                }
                _ => {
                    let volume = device
                        .volume
                        .adjusted(matches!(action, Action::MicUp | Action::VolumeUp));
                    if microphone {
                        pa_context_set_source_volume_by_index(
                            self.context,
                            device.index,
                            &volume,
                            success,
                            data,
                        )
                    } else {
                        pa_context_set_sink_volume_by_index(
                            self.context,
                            device.index,
                            &volume,
                            success,
                            data,
                        )
                    }
                }
            }
        };
        if op.is_null() {
            return Err(self.error());
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        let result = loop {
            if let Err(e) = self.iterate() {
                break Err(e);
            }
            if let Some(ok) = self.state.ack {
                break if ok { Ok(()) } else { Err(self.error()) };
            }
            if Instant::now() >= deadline {
                break Err("audio control timed out".into());
            }
            thread::sleep(Duration::from_millis(5));
        };
        unsafe {
            if result.is_err() {
                pa_operation_cancel(op);
            }
            pa_operation_unref(op);
        }
        self.query();
        result
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        self.stop_stream();
        unsafe {
            if !self.context.is_null() {
                pa_context_disconnect(self.context);
                pa_context_unref(self.context);
            }
            if !self.mainloop.is_null() {
                pa_mainloop_free(self.mainloop);
            }
        }
        // Box<State> outlives disconnect/unref and all their callbacks.
    }
}

// Monitor exactly one MPD sink input. Never substitute the default microphone or
// a sink's combined monitor. Native PipeWire MPD lacks process.binary metadata.
#[repr(C)]
struct SinkInputInfo {
    index: u32,
    name: *const c_char,
    owner: u32,
    client: u32,
    sink: u32,
    sample: SampleSpec,
    map: ChannelMap,
    volume: Volume,
    buffer_usec: u64,
    sink_usec: u64,
    resample: *const c_char,
    driver: *const c_char,
    mute: c_int,
    properties: Handle,
}
#[link(name = "pulse")]
unsafe extern "C" {
    fn pa_context_get_sink_input_info_list(
        c: Handle,
        cb: unsafe extern "C" fn(Handle, *const SinkInputInfo, c_int, Handle),
        data: Handle,
    ) -> Handle;
    fn pa_context_get_sink_info_by_index(
        c: Handle,
        index: u32,
        cb: InfoCallback,
        data: Handle,
    ) -> Handle;
    fn pa_context_get_source_info_by_index(
        c: Handle,
        index: u32,
        cb: InfoCallback,
        data: Handle,
    ) -> Handle;
    fn pa_proplist_gets(p: Handle, key: *const c_char) -> *const c_char;
    fn pa_context_get_client_info(
        c: Handle,
        index: u32,
        cb: unsafe extern "C" fn(Handle, *const MusicOwnerInfo, c_int, Handle),
        data: Handle,
    ) -> Handle;
    fn pa_stream_set_monitor_stream(s: Handle, index: u32) -> c_int;
}
#[repr(C)]
struct MusicOwnerInfo {
    index: u32,
    name: *const c_char,
    owner: u32,
    driver: *const c_char,
    properties: Handle,
}
#[derive(Clone, PartialEq, Eq)]
struct MusicTarget {
    input: u32,
    source: String,
}
#[derive(Default)]
struct MusicSelector {
    process: Option<u32>,
    pending: std::collections::VecDeque<(u32, u32, u32, Option<u32>)>,
    checking: Option<(u32, u32)>,
    overflow: bool,
    candidate: Option<(u32, u32)>,
    count: usize,
    target: Option<MusicTarget>,
    done: bool,
    dirty: bool,
}
unsafe extern "C" fn music_events(_: Handle, _: u32, _: u32, data: Handle) {
    (*(data as *mut MusicSelector)).dirty = true;
}
unsafe fn property(p: Handle, key: &CStr) -> Option<&str> {
    if p.is_null() {
        return None;
    }
    let s = pa_proplist_gets(p, key.as_ptr());
    if s.is_null() {
        None
    } else {
        CStr::from_ptr(s).to_str().ok()
    }
}
unsafe extern "C" fn music_inputs(c: Handle, i: *const SinkInputInfo, eol: c_int, data: Handle) {
    let state = &mut *(data as *mut MusicSelector);
    if eol != 0 {
        if eol < 0 || state.overflow {
            state.done = true;
            return;
        }
        choose_music_owner(c, data);
    } else if let Some(i) = i.as_ref() {
        let matched = property(i.properties, c"application.process.binary") == Some("mpd")
            || property(i.properties, c"application.name") == Some("Music Player Daemon")
            || property(i.properties, c"node.name").is_some_and(|s| s.starts_with("mpd."));
        if matched {
            if state.pending.len() >= 16 {
                state.overflow = true;
                return;
            }
            state.pending.push_back((
                i.index,
                i.sink,
                i.client,
                property(i.properties, c"application.process.id").and_then(|s| s.parse().ok()),
            ));
        }
    }
}
unsafe fn choose_music_owner(c: Handle, data: Handle) {
    let state = &mut *(data as *mut MusicSelector);
    while let Some((input, sink, client, pid)) = state.pending.pop_front() {
        if state.process.is_none() || pid == state.process {
            state.count += 1;
            state.candidate = Some((input, sink));
            continue;
        }
        if pid.is_some() || client == u32::MAX {
            continue;
        }
        state.checking = Some((input, sink));
        let op = pa_context_get_client_info(c, client, music_owner, data);
        if !op.is_null() {
            forget(op);
            return;
        }
        state.checking = None;
    }
    if state.count == 1 {
        let (_, sink) = state.candidate.unwrap();
        let op = pa_context_get_sink_info_by_index(c, sink, music_sink, data);
        if op.is_null() {
            state.done = true;
        }
        forget(op);
    } else {
        state.done = true;
    }
}
unsafe extern "C" fn music_owner(c: Handle, i: *const MusicOwnerInfo, eol: c_int, data: Handle) {
    let state = &mut *(data as *mut MusicSelector);
    if let Some(i) = i.as_ref() {
        if property(i.properties, c"application.process.id").and_then(|s| s.parse::<u32>().ok())
            == state.process
        {
            state.count += 1;
            state.candidate = state.checking;
        }
    }
    if eol != 0 {
        state.checking = None;
        choose_music_owner(c, data);
    }
}
unsafe extern "C" fn music_sink(c: Handle, i: *const DeviceInfo, eol: c_int, data: Handle) {
    let state = &mut *(data as *mut MusicSelector);
    if let Some(i) = i.as_ref() {
        if i.monitor == u32::MAX {
            state.done = true;
            return;
        }
        let op = pa_context_get_source_info_by_index(c, i.monitor, music_source, data);
        if op.is_null() {
            state.done = true;
        }
        forget(op);
    } else if eol < 0 {
        state.done = true;
    }
}
unsafe extern "C" fn music_source(_: Handle, i: *const DeviceInfo, eol: c_int, data: Handle) {
    let state = &mut *(data as *mut MusicSelector);
    if let Some(i) = i.as_ref() {
        let source = string(i.name);
        if source.len() <= 1024 {
            if let Some((input, _)) = state.candidate {
                state.target = Some(MusicTarget { input, source });
            }
        }
    }
    if eol != 0 {
        state.done = true;
    }
}
/// Field order is significant: client destroys native callbacks before selector.
pub(super) struct MusicClient {
    client: Client,
    selector: Box<MusicSelector>,
    target: Option<MusicTarget>,
    queried: Instant,
    retry: Instant,
}
impl MusicClient {
    pub fn connect_for_process(process: Option<u32>) -> Result<Self, String> {
        let client = Client::connect_inner(None, Duration::from_millis(300), false)?;
        let mut out = Self {
            client,
            selector: Box::new(MusicSelector {
                process,
                ..Default::default()
            }),
            target: None,
            queried: Instant::now(),
            retry: Instant::now(),
        };
        let data = out.data();
        unsafe {
            pa_context_set_subscribe_callback(out.client.context, music_events, data);
            forget(pa_context_subscribe(
                out.client.context,
                0x7,
                None,
                ptr::null_mut(),
            ));
        }
        out.query();
        Ok(out)
    }
    fn data(&mut self) -> Handle {
        (&mut *self.selector as *mut MusicSelector).cast()
    }
    fn query(&mut self) {
        self.selector.candidate = None;
        self.selector.pending.clear();
        self.selector.checking = None;
        self.selector.overflow = false;
        self.selector.count = 0;
        self.selector.target = None;
        self.selector.done = false;
        self.selector.dirty = false;
        self.queried = Instant::now();
        let data = self.data();
        let op =
            unsafe { pa_context_get_sink_input_info_list(self.client.context, music_inputs, data) };
        if op.is_null() {
            self.selector.done = true;
        }
        unsafe { forget(op) };
    }
    pub fn tick(&mut self) -> Result<Option<f32>, String> {
        self.client.iterate()?;
        if !self.selector.done && self.queried.elapsed() > Duration::from_millis(600) {
            return Err("MPD audio stream query timed out".into());
        }
        if self.selector.done {
            let selected = self.selector.target.clone();
            if selected != self.target {
                self.client.stop_stream();
                self.target = selected;
                self.retry = Instant::now();
            }
            if self.client.stream.is_null() && Instant::now() >= self.retry {
                if let Some(target) = &self.target {
                    let name = CString::new(target.source.as_str())
                        .map_err(|_| "invalid MPD monitor name")?;
                    let spec = SampleSpec {
                        format: 5,
                        rate: 25,
                        channels: 1,
                    };
                    unsafe {
                        self.client.stream = pa_stream_new(
                            self.client.context,
                            c"g13pad MPD peaks".as_ptr(),
                            &spec,
                            ptr::null_mut(),
                        );
                        if !self.client.stream.is_null() {
                            let data = self.client.data();
                            pa_stream_set_read_callback(self.client.stream, peaks, data);
                            let attr = BufferAttr {
                                maxlength: u32::MAX,
                                tlength: u32::MAX,
                                prebuf: u32::MAX,
                                minreq: u32::MAX,
                                fragsize: 4,
                            };
                            if pa_stream_set_monitor_stream(self.client.stream, target.input) < 0
                                || pa_stream_connect_record(
                                    self.client.stream,
                                    name.as_ptr(),
                                    &attr,
                                    0x0800 | 0x0200 | 0x2000,
                                ) < 0
                            {
                                self.client.stop_stream();
                            }
                        }
                    }
                    self.retry = Instant::now() + Duration::from_secs(3);
                }
            }
            if self.selector.dirty || self.queried.elapsed() > Duration::from_secs(2) {
                self.query();
            }
        }
        if !self.client.stream.is_null() && unsafe { pa_stream_get_state(self.client.stream) } >= 3
        {
            self.client.stop_stream();
            self.retry = Instant::now() + Duration::from_secs(3);
        }
        Ok(self.client.peak())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires a live audio server and opens its default microphone peak stream"]
    fn peak_stream_keeps_device_queries_responsive() {
        let mut client = Client::connect(None).unwrap();
        let name = client.devices().0.expect("default microphone").name;
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(40) {
            client.tick(true).unwrap();
            assert_eq!(
                client.devices().0.expect("microphone remains present").name,
                name
            );
            thread::sleep(Duration::from_millis(20));
        }
    }
    #[test]
    fn volume_changes_preserve_balance_stop_at100_and_do_not_lower_boost_on_up() {
        let mut v = Volume {
            channels: 2,
            values: [0; 32],
        };
        v.values[0] = 32768;
        v.values[1] = 16384;
        let up = v.adjusted(true);
        assert_eq!(up.maximum(), 36045);
        assert!((up.values[0] as i64 - up.values[1] as i64 * 2).abs() <= 1);
        v.values[0] = 65000;
        assert_eq!(v.adjusted(true).maximum(), 65536);
        v.values[0] = 90000;
        assert_eq!(v.adjusted(true).maximum(), 90000);
        v.values.fill(0);
        assert_eq!(v.adjusted(false).maximum(), 0);
        assert_eq!(v.adjusted(true).maximum(), 3277);
    }
}

#[cfg(test)]
mod music_tests {
    use super::*;
    #[test]
    #[ignore = "opens only the identified MPD output peak monitor; requires a running audio server/MPD stream"]
    fn music_stream_produces_native_peak_samples() {
        let pid = std::env::var("G13MAP_MPD_TEST_PROCESS_ID")
            .ok()
            .and_then(|s| s.parse().ok());
        let mut client = MusicClient::connect_for_process(pid).unwrap();
        let mut maximum = 0f32;
        let deadline = Instant::now() + Duration::from_secs(4);
        let mut received = false;
        while Instant::now() < deadline {
            if let Some(p) = client.tick().unwrap() {
                received = true;
                maximum = maximum.max(p);
            }
            thread::sleep(Duration::from_millis(20));
        }
        eprintln!("MPD monitored maximum peak: {maximum}");
        if let Ok(limit) = std::env::var("G13MAP_MPD_TEST_MAX_PEAK") {
            assert!(
                maximum <= limit.parse::<f32>().unwrap(),
                "other stream leaked into MPD monitor"
            );
        }
        assert!(
            received,
            "identified MPD stream produced no native peak samples"
        );
    }
}
