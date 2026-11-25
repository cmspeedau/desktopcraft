//! Clickable native Explorer Tab controller (experimental).
//! No imitation icons or shell browser: only a small temporary Tab header.
#![cfg(windows)]
use std::{cell::{Cell,RefCell},collections::VecDeque,mem,ptr};
use winapi::shared::minwindef::{LPARAM,LRESULT,UINT,WPARAM};
use winapi::shared::windef::{HWND,HMENU};
use winapi::um::libloaderapi::GetModuleHandleW;
use winapi::um::winuser::*;

const TAB_A:usize=0x4DA1;
const TAB_B:usize=0x4DA2;
const STOP:usize=0x4DA3;
#[derive(Debug,Clone,Copy,PartialEq,Eq)]
pub enum Action { SelectA,SelectB,Stop }
thread_local! {
 static EVENTS:RefCell<VecDeque<Action>>=const {RefCell::new(VecDeque::new())};
 static CLOSED:Cell<bool>=const {Cell::new(false)};
}
fn wide(s:&str)->Vec<u16>{s.encode_utf16().chain(Some(0)).collect()}
unsafe extern "system" fn proc(hwnd:HWND,msg:UINT,wp:WPARAM,lp:LPARAM)->LRESULT {
 match msg {
   WM_COMMAND=>{
     let action=match wp&0xffff {
       TAB_A=>Some(Action::SelectA),TAB_B=>Some(Action::SelectB),
       STOP=>Some(Action::Stop),_=>None,
     };
     if let Some(action)=action {EVENTS.with(|q|q.borrow_mut().push_back(action));return 0;}
   }
   WM_CLOSE=>{
     EVENTS.with(|q|q.borrow_mut().push_back(Action::Stop));
     DestroyWindow(hwnd);
     return 0;
   }
   WM_DESTROY=>{CLOSED.with(|v|v.set(true));return 0;}
   _=>{}
 }
 DefWindowProcW(hwnd,msg,wp,lp)
}
pub struct NativeTabPanel { pub hwnd:HWND }
impl NativeTabPanel {
 pub fn open(from:&str,to:&str,group:u64)->Result<Self,String>{
   EVENTS.with(|queue|queue.borrow_mut().clear());
   CLOSED.with(|flag|flag.set(false));
   unsafe{
     let instance=GetModuleHandleW(ptr::null());
     let class=wide("DesktopCraftNativeTabLiveController");
     let wc=WNDCLASSEXW{
       cbSize:mem::size_of::<WNDCLASSEXW>() as u32,
       hInstance:instance,lpfnWndProc:Some(proc),
       hCursor:LoadCursorW(ptr::null_mut(),IDC_ARROW),
       hbrBackground:(COLOR_WINDOW+1) as _,
       lpszClassName:class.as_ptr(),..mem::zeroed()
     };
     RegisterClassExW(&wc);
     let x=(GetSystemMetrics(SM_CXSCREEN)-490).max(20);
     let title=wide(&format!("DesktopCraft Native Tab Lab - Box {group}"));
     let hwnd=CreateWindowExW(WS_EX_TOOLWINDOW,
       class.as_ptr(),title.as_ptr(),WS_OVERLAPPED|WS_CAPTION|WS_SYSMENU,
       x,100,480,142,ptr::null_mut(),ptr::null_mut(),instance,ptr::null_mut());
     if hwnd.is_null(){return Err(format!("Native Tab panel creation failed: {}",std::io::Error::last_os_error()));}
     let button=|id:usize,x:i32,width:i32,label:&str|->bool{
       let label=wide(label);
       !CreateWindowExW(0,wide("BUTTON").as_ptr(),label.as_ptr(),
         WS_CHILD|WS_VISIBLE|WS_TABSTOP|BS_PUSHBUTTON,
         x,18,width,50,hwnd,id as HMENU,instance,ptr::null_mut()).is_null()
     };
     if !button(TAB_A,12,165,from)||!button(TAB_B,184,165,to)||!button(STOP,356,110,"Stop"){
       DestroyWindow(hwnd);return Err("Native Tab buttons could not be created".into());
     }
     ShowWindow(hwnd,SW_SHOWNORMAL);
     UpdateWindow(hwnd);
     Ok(Self{hwnd})
   }
 }
 pub fn poll(&self)->Vec<Action>{
   unsafe{
     let mut msg:MSG=mem::zeroed();
     while PeekMessageW(&mut msg,ptr::null_mut(),0,0,PM_REMOVE)!=0{
       TranslateMessage(&msg);DispatchMessageW(&msg);
     }
   }
   EVENTS.with(|q|q.borrow_mut().drain(..).collect())
 }
 pub fn closed(&self)->bool{CLOSED.with(|v|v.get())}
 /// Synthetic UI event only for automated integration checks.
 pub fn simulate_button_for_test(&self,to_tab:usize)->bool{
   unsafe{PostMessageW(self.hwnd,WM_COMMAND,if to_tab==0{TAB_A}else{TAB_B},0)!=0}
 }
}
impl Drop for NativeTabPanel {
 fn drop(&mut self){
  unsafe{if IsWindow(self.hwnd)!=0{DestroyWindow(self.hwnd);}}
 }
}
