#![no_std]
#![no_main]
use cmf_wp2_app::{Api,storage::Storage};
cmf_wp2_app::entry!(main);
#[unsafe(no_mangle)]
static mut STORAGE_PROBE:[u32;8]=[0;8];
fn main(api:&'static Api)->i32 {
 let s=match Storage::open(api){Ok(s)=>s,Err(e)=>return e};
 let c=match s.capabilities(){Ok(c)=>c,Err(e)=>return e};
 let f=match s.filesystem(){Ok(f)=>f,Err(e)=>return e};
 if let Err(e)=s.begin_scan(){return e;}
 let step=match s.scan_step(){Ok(p)=>p,Err(e)=>return e};
 #[cfg(not(feature="icons"))]
 {api.rect(0,0,466,466,0);api.text(100,200,"Storage probe",0xffff);(api.present)();}
 #[cfg(feature="icons")]
 {
  use cmf_wp2_app::{lv::*,stock};
  let lv=api.lv().unwrap();let mut b=Batch::new().unwrap();stock::font(&mut b).unwrap();b.submit(&lv).unwrap();stock::theme(&mut b).unwrap();
  for (slot,side,x,y) in [(0,IconSize::Thumbnail,90,180),(8,IconSize::Native,240,164),(15,IconSize::Thumbnail,190,300)] {
   let o=b.create(Class::Obj,ROOT).unwrap();b.size(o,side as i32,side as i32).unwrap();
   b.align(o,Align::TopLeft,x,y).unwrap();b.style(o,Prop::BgOpa,0,0).unwrap();
   b.installed_app_icon(o,slot,side).unwrap();
  }
  b.submit(&lv).unwrap();
 }

 api.own_back(true);
 unsafe{core::ptr::addr_of_mut!(STORAGE_PROBE).write_volatile([if cfg!(feature="icons"){2}else{1},c.version as u32,c.app_slots as u32,f.total,f.free,step.files as u32,step.complete as u32,step.partial as u32]);}
 // Deliberately hang with an open scan. The loader must enforce its existing back escape and free the scan.
 loop{core::hint::spin_loop();}
}
