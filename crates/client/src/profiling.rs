//! Opt-in logical client GPU ownership and CPU work durations. GPU bytes are
//! descriptor payload sizes, not driver VRAM: exclude padding, pipelines,
//! bind groups, host UI resources, pending submissions and externally retained
//! views after their backend is dropped. Enable before constructing clients.
use std::sync::{Mutex, atomic::{AtomicBool, AtomicU64, Ordering::Relaxed}};
use std::time::Instant;
static ENABLED:AtomicBool=AtomicBool::new(false);
pub fn enable() { ENABLED.store(true,Relaxed); }
#[derive(Clone,Copy,Default,Debug)]
pub struct GpuBytes { pub buffers:u64, pub textures:u64, pub peak:u64 }
static GPU:Mutex<GpuBytes>=Mutex::new(GpuBytes{buffers:0,textures:0,peak:0});
pub fn gpu_bytes()->GpuBytes { *GPU.lock().unwrap() }
pub struct Allocation { buffers:u64,textures:u64 }
impl Allocation {
    pub fn new(buffers:u64,textures:u64)->Self {
        let (buffers,textures)=if ENABLED.load(Relaxed){(buffers,textures)}else{(0,0)};
        if buffers+textures>0 { let mut g=GPU.lock().unwrap();g.buffers+=buffers;g.textures+=textures;g.peak=g.peak.max(g.buffers+g.textures); }
        Self{buffers,textures}
    }
}
impl Drop for Allocation {
    fn drop(&mut self) { if self.buffers+self.textures>0 { let mut g=GPU.lock().unwrap();g.buffers-=self.buffers;g.textures-=self.textures; } }
}
/// Count every mip and array layer; current formats have 4-byte texels
/// (including Depth32Float). Unknown format sizes fail visibly in profiling.
pub fn texture_bytes(t:&wgpu::Texture)->u64 {
    if !ENABLED.load(Relaxed) { return 0; }
    let bytes=t.format().block_copy_size(None).expect("profile: texture format size");
    let (bw,bh)=t.format().block_dimensions();
    (0..t.mip_level_count()).map(|m| {
        let w=(t.width()>>m).max(1).div_ceil(bw) as u64;
        let h=(t.height()>>m).max(1).div_ceil(bh) as u64;
        let layers=if t.dimension()==wgpu::TextureDimension::D3 {(t.depth_or_array_layers()>>m).max(1)}else{t.depth_or_array_layers()};
        w*h*layers as u64*bytes as u64*t.sample_count() as u64
    }).sum()
}
pub struct DurationCounter { count:AtomicU64,total:AtomicU64,max:AtomicU64 }
impl DurationCounter {
    pub const fn new()->Self { Self{count:AtomicU64::new(0),total:AtomicU64::new(0),max:AtomicU64::new(0)} }
    pub fn start(&'static self)->Timer { Timer{counter:self,start:ENABLED.load(Relaxed).then(Instant::now)} }
    /// Cumulative (count,total nanoseconds,max nanoseconds); consumers can
    /// difference count/total between samples without resetting hot counters.
    pub fn read(&self)->(u64,u64,u64) { (self.count.load(Relaxed),self.total.load(Relaxed),self.max.load(Relaxed)) }
}
pub struct Timer { counter:&'static DurationCounter,start:Option<Instant> }
impl Drop for Timer { fn drop(&mut self) { if let Some(start)=self.start {let ns=start.elapsed().as_nanos().min(u64::MAX as u128) as u64;self.counter.total.fetch_add(ns,Relaxed);self.counter.max.fetch_max(ns,Relaxed);self.counter.count.fetch_add(1,Relaxed);} } }
pub static CLIENT_TICK:DurationCounter=DurationCounter::new();
pub static UI_DRAW:DurationCounter=DurationCounter::new();
pub static UI_FRAME:DurationCounter=DurationCounter::new();
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn allocation_replacement_and_drop_release_only_their_bytes() {
        enable();let before=gpu_bytes();
        let old=Allocation::new(16,32);let new=Allocation::new(64,128);
        let during=gpu_bytes();assert_eq!(during.buffers-before.buffers,80);assert_eq!(during.textures-before.textures,160);
        drop(old);let after=gpu_bytes();assert_eq!(after.buffers-before.buffers,64);assert_eq!(after.textures-before.textures,128);
        drop(new);assert_eq!(gpu_bytes().buffers,before.buffers);assert_eq!(gpu_bytes().textures,before.textures);
    }
}
