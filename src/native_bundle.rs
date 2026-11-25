//! Windows EXE-embedded version-matched native Explorer filter DLL.
//! Used ONLY by explicitly gated `native-release` candidate builds.
//! The built executable is a self-contained setup, as in prior releases.
use std::{ffi::c_void,fs,io::Write,path::PathBuf};
#[link(name="kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(name:*const u16)->*mut c_void;
    fn FindResourceW(module:*mut c_void,name:*const u16,kind:*const u16)->*mut c_void;
    fn SizeofResource(module:*mut c_void,resource:*mut c_void)->u32;
    fn LoadResource(module:*mut c_void,resource:*mut c_void)->*mut c_void;
    fn LockResource(global:*mut c_void)->*const u8;
}
pub fn embedded_filter()->Result<&'static [u8],String>{
    if !cfg!(feature="native-release") {
        return Err("Native production DLL not embedded in this build".into());
    }
    unsafe {
        let h=GetModuleHandleW(std::ptr::null());
        if h.is_null(){return Err("Cannot locate installed executable PE module".into());}
        let res=FindResourceW(h,200usize as *const u16,10usize as *const u16);
        if res.is_null(){return Err("Embedded native-filter PE resource missing".into());}
        let size=SizeofResource(h,res) as usize;
        if !(20_000..=20_000_000).contains(&size){
            return Err("Embedded Native DLL size outside expected limits".into());
        }
        let ptr=LockResource(LoadResource(h,res));
        if ptr.is_null(){return Err("Embedded native-filter PE resource unreadable".into());}
        let data=std::slice::from_raw_parts(ptr,size);
        if !data.starts_with(b"MZ") {
            return Err("Embedded DLL is not a Windows PE binary".into());
        }
        Ok(data)
    }
}
fn bundle_directory_name(pid:u32,created:u64)->String {
    format!("DesktopCraftNativeBundle-{pid}-{created}")
}
pub fn extract_native_filter()->Result<PathBuf,String>{
    let embedded = embedded_filter()?;
    let stamp=crate::app_startup_guards::process_creation_stamp(unsafe {
        winapi::um::processthreadsapi::GetCurrentProcess()
    }).ok_or("Native bundle owner creation time unavailable")?;
    let directory=std::env::temp_dir()
        .join(bundle_directory_name(std::process::id(),stamp));
    fs::create_dir_all(&directory).map_err(|e|e.to_string())?;
    let target=directory.join("desktopcraft_native_filter.dll");
    match fs::OpenOptions::new().create_new(true).write(true).open(&target){
            Ok(mut file)=>file.write_all(embedded)
                .and_then(|()|file.sync_all())
                .map_err(|e|format!("Cannot unpack native filter DLL: {e}"))?,
            Err(e) if e.kind()==std::io::ErrorKind::AlreadyExists=>{
                let existing=fs::read(&target).map_err(|e|e.to_string())?;
                if existing!=embedded{
                    return Err("Previously extracted native DLL content differs from embedded PE resource".into());
                }
            }
            Err(error)=>return Err(error.to_string()),
    }
    Ok(target)
}

#[cfg(test)]
mod tests {
    #[test]
    fn native_bundle_directory_distinguishes_reused_process_ids() {
        let first=super::bundle_directory_name(1234,100);
        assert_eq!(first,super::bundle_directory_name(1234,100));
        assert_ne!(first,super::bundle_directory_name(1234,101));
        assert_ne!(first,super::bundle_directory_name(1235,100));
    }
}
