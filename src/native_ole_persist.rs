//! Conditional persisted OLE Virtual Pin transaction.
//!
//! This is a DISCONNECTED laboratory bridge: it neither registers an HWND nor
//! connects to production Explorer. All tests use a disposable workspace.
//! The named sidecar lease coordinates only code paths that honor the same
//! lease. Existing uncooperative State::save callers are NOT yet protected.
use crate::explorer_native::NativeMultiBoxPlan;
use crate::model::{State, MAX_WORKSPACE_BYTES};
use crate::native_ole_gate::{NativeOleDropTicket, NativeOleViewport};
use crate::shell::ViewItem;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn disk_error(what: &str, error: std::io::Error) -> String {
    format!("{what}: {error}")
}

fn lock_path(path: &Path) -> PathBuf {
    path.with_extension("ole-lock")
}
fn backup_path(path: &Path) -> PathBuf {
    path.with_extension("ole-backup")
}

/// New-format stage filenames include the specific Workspace basename.
/// This distinguishes unrelated files in the same folder. Legacy unqualified
/// stages cannot safely be assigned to a workspace and are never deleted.
fn workspace_stage_prefix(path:&Path)->Result<String,String>{
    let name=path.file_name().ok_or("Workspace stage has no filename")?;
    let name=name.to_str().ok_or("Workspace filename is not UTF-8")?;
    Ok(format!(".{name}.desktopcraft-ole-"))
}

/// Read-only, bounded stage discovery. No automatic cleanup or recovery.
pub(crate) fn unresolved_workspace_ole_stages(path:&Path)->Result<Vec<PathBuf>,String>{
    let parent=path.parent().ok_or("Workspace stage has no parent directory")?;
    let prefix=workspace_stage_prefix(path)?;
    let mut found=Vec::new();
    for entry in fs::read_dir(parent)
        .map_err(|e|disk_error("Could not inspect OLE staging directory",e))? {
        let entry=entry.map_err(|e|disk_error("Could not inspect OLE staging entry",e))?;
        let file_name=entry.file_name();
        let Some(name)=file_name.to_str() else {continue;};
        if name.starts_with(&prefix) && name.ends_with(".tmp") {
            found.push(entry.path());
            if found.len()>32 {
                return Err("OLE staging directory has too many pending recovery files".into());
            }
        }
    }
    found.sort();
    Ok(found)
}

/// On Windows the kernel owns the lease lifetime: exclusive sharing denies
/// any second process from acquiring it while open, and DELETE_ON_CLOSE makes
/// a killed/crashed process release its lock instead of permanently blocking
/// DesktopCraft saves. OPEN_ALWAYS also tolerates an orphaned lock from an
/// older version; this never modifies the actual Workspace or its backup.
#[cfg(windows)]
fn open_workspace_lease(lock: &Path) -> Result<File, String> {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::FromRawHandle;
    #[link(name = "kernel32")]
    extern "system" {
        fn CreateFileW(
            name: *const u16,
            access: u32,
            share: u32,
            security: *mut c_void,
            disposition: u32,
            flags: u32,
            template: *mut c_void,
        ) -> *mut c_void;
    }
    const GENERIC_READ: u32 = 0x8000_0000;
    const GENERIC_WRITE: u32 = 0x4000_0000;
    const DELETE: u32 = 0x0001_0000;
    const OPEN_ALWAYS: u32 = 4;
    const FILE_ATTRIBUTE_TEMPORARY: u32 = 0x0000_0100;
    const FILE_FLAG_DELETE_ON_CLOSE: u32 = 0x0400_0000;
    let wide = lock
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            GENERIC_READ | GENERIC_WRITE | DELETE,
            0,
            std::ptr::null_mut(),
            OPEN_ALWAYS,
            FILE_ATTRIBUTE_TEMPORARY | FILE_FLAG_DELETE_ON_CLOSE,
            std::ptr::null_mut(),
        )
    };
    if handle as isize == -1 {
        return Err(disk_error(
            "Windows exclusive crash-safe OLE lease unavailable",
            std::io::Error::last_os_error(),
        ));
    }
    Ok(unsafe { File::from_raw_handle(handle) })
}
#[cfg(not(windows))]
fn open_workspace_lease(lock: &Path) -> Result<File, String> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(lock)
        .map_err(|e| disk_error("Workspace OLE lease already held", e))
}

pub(crate) struct WorkspaceLease {
    path: PathBuf,
    file: Option<File>,
}
impl WorkspaceLease {
    pub(crate) fn acquire(path: &Path) -> Result<Self, String> {
        let lock = lock_path(path);
        let file = open_workspace_lease(&lock)?;
        Ok(Self {
            path: lock,
            file: Some(file),
        })
    }
}
impl Drop for WorkspaceLease {
    fn drop(&mut self) {
        // Windows cannot delete a held file handle; close before unlink.
        self.file.take();
        let _ = fs::remove_file(&self.path);
    }
}

struct StageFile {
    path: PathBuf,
    file: Option<File>,
}
impl StageFile {
    fn new(workspace: &Path) -> Result<Self, String> {
        let parent = workspace
            .parent()
            .ok_or("Workspace has no parent directory")?;
        let prefix=workspace_stage_prefix(workspace)?;
        for attempt in 0..4u8 {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_nanos();
            let path = parent.join(format!(
                "{prefix}{}-{stamp}-{attempt}.tmp",
                std::process::id()
            ));
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => {
                    return Ok(Self {
                        path,
                        file: Some(file),
                    })
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(disk_error("Cannot create an OLE staging file", e)),
            }
        }
        Err("Unable to allocate a unique workspace OLE staging file".into())
    }
    fn stage(&mut self, bytes: &[u8]) -> Result<(), String> {
        let file = self.file.as_mut().ok_or("OLE stage is already closed")?;
        file.write_all(bytes)
            .map_err(|e| disk_error("Could not write OLE stage", e))?;
        file.sync_all()
            .map_err(|e| disk_error("Could not flush OLE stage", e))?;
        self.file.take(); // close before Windows ReplaceFileW
        Ok(())
    }
}
impl Drop for StageFile {
    fn drop(&mut self) {
        self.file.take();
        let _ = fs::remove_file(&self.path); // no-op after successful replace
    }
}

#[cfg(windows)]
fn replace_preserving_backup(workspace: &Path, stage: &Path, backup: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" {
        fn ReplaceFileW(
            replaced: *const u16,
            replacement: *const u16,
            backup: *const u16,
            flags: u32,
            exclude: *mut std::ffi::c_void,
            reserved: *mut std::ffi::c_void,
        ) -> i32;
    }
    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }
    let a = wide(workspace);
    let b = wide(stage);
    let c = wide(backup);
    // Destination must exist: never create or rewrite a missing user Workspace.
    let ok = unsafe {
        ReplaceFileW(
            a.as_ptr(),
            b.as_ptr(),
            c.as_ptr(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(disk_error(
            "Windows ReplaceFileW rejected OLE commit",
            std::io::Error::last_os_error(),
        ));
    }
    Ok(())
}
#[cfg(not(windows))]
fn replace_preserving_backup(workspace: &Path, stage: &Path, backup: &Path) -> Result<(), String> {
    fs::copy(workspace, backup).map_err(|e| disk_error("Could not create OLE backup", e))?;
    fs::rename(stage, workspace).map_err(|e| disk_error("Could not rename OLE stage", e))
}


/// ReplaceFileW records the exact overwritten on-disk predecessor in the
/// backup. Compare it with DragEnter's original bytes: this catches even an
/// *uncooperative* external writer racing after our last read but before
/// ReplaceFileW. Never auto-restore on mismatch; preserve backup for review.
fn verify_replaced_predecessor(backup:&Path, expected:&[u8])->Result<(),String>{
    let actual=read_existing(backup)?;
    if actual!=expected {
        return Err("Uncooperative Workspace writer raced with OLE ReplaceFileW; overwritten predecessor backup retained for manual recovery".into());
    }
    Ok(())
}

pub(crate) fn read_existing(path: &Path) -> Result<Vec<u8>, String> {
    let file = OpenOptions::new()
        .read(true)
        .open(path)
        .map_err(|e| disk_error("Could not read existing Workspace", e))?;
    let mut bytes = Vec::new();
    // Bound allocation *while reading*, not just after completion.
    file.take(MAX_WORKSPACE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| disk_error("Could not read bounded Workspace", e))?;
    if bytes.is_empty() || bytes.len() > MAX_WORKSPACE_BYTES {
        return Err("OLE Workspace is empty or exceeds bounded size".into());
    }
    Ok(bytes)
}
fn serialized_state(state: &State) -> Result<Vec<u8>, String> {
    serde_json::to_vec(state).map_err(|e| format!("Invalid Workspace state: {e}"))
}

/// Non-mutating crash-artifact inspection. A leftover lease may belong to a
/// live writer, and a valid backup may hold intentional older layout data:
/// never delete or restore either automatically.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeOleRecoveryReport {
    pub workspace_exists: bool,
    pub workspace_valid: bool,
    pub backup_exists: bool,
    pub backup_valid: bool,
    /// Crash backup from an interrupted native destination Box commit.
    pub native_target_backup_exists: bool,
    pub native_target_backup_valid: bool,
    /// The overwritten post-image retained after explicit conditional rollback.
    pub rollback_after_exists: bool,
    pub rollback_after_valid: bool,
    pub lease_present: bool,
    /// Remaining new-format stages for this exact workspace (never deleted).
    pub staged_artifact_count: usize,
    /// The one pending stage, when unambiguous, parses as a saved State.
    pub single_stage_valid: bool,
    /// False when parent directory inspection failed or overflowed.
    pub stage_scan_ok: bool,
    pub needs_manual_review: bool,
}

pub fn inspect_native_ole_recovery(workspace: &Path) -> NativeOleRecoveryReport {
    let backup = backup_path(workspace);
    let native_backup=workspace.with_extension("native-target-backup");
    let rolled_back=workspace.with_extension("rollback-after");
    let native_target_backup_exists=native_backup.is_file();
    let rollback_after_exists=rolled_back.is_file();
    let native_target_backup_valid=native_target_backup_exists &&
        read_existing(&native_backup)
            .and_then(|bytes|State::from_saved_bytes(&bytes).map(|_|())).is_ok();
    let rollback_after_valid=rollback_after_exists &&
        read_existing(&rolled_back)
            .and_then(|bytes|State::from_saved_bytes(&bytes).map(|_|())).is_ok();
    let lease_present = lock_path(workspace).exists();
    let workspace_exists = workspace.is_file();
    let backup_exists = backup.is_file();
    let workspace_valid=workspace_exists && read_existing(workspace)
        .and_then(|bytes|State::from_saved_bytes(&bytes).map(|_|())).is_ok();
    let backup_valid=backup_exists && read_existing(&backup)
        .and_then(|bytes|State::from_saved_bytes(&bytes).map(|_|())).is_ok();
    let stages=unresolved_workspace_ole_stages(workspace);
    let stage_scan_ok=stages.is_ok();
    let staged_artifact_count=stages.as_ref().map_or(0,|items|items.len());
    let single_stage_valid=stages.as_ref().is_ok_and(|items|
        items.len()==1 && read_existing(&items[0])
            .and_then(|bytes|State::from_saved_bytes(&bytes).map(|_|())).is_ok());
    NativeOleRecoveryReport {
        workspace_exists,
        workspace_valid,
        backup_exists,
        backup_valid,
        native_target_backup_exists,
        native_target_backup_valid,
        rollback_after_exists,
        rollback_after_valid,
        lease_present,
        staged_artifact_count,
        single_stage_valid,
        stage_scan_ok,
        needs_manual_review: !workspace_valid || backup_exists || lease_present
            || native_target_backup_exists || rollback_after_exists
            || staged_artifact_count>0 || !stage_scan_ok,
    }
}


/// Common copy-only export for verified backups and interrupted stage files.
/// Never writes the current Workspace, original backup, or source stage.
fn copy_validated_recovery_candidate(source:&Path,destination:&Path)->Result<(),String>{
    let bytes=read_existing(source)?;
    State::from_saved_bytes(&bytes)
        .map_err(|e|format!("OLE recovery candidate is not a valid DesktopCraft State: {e}"))?;
    let mut output=OpenOptions::new()
        .write(true).create_new(true).open(destination)
        .map_err(|e|disk_error("Recovery export destination is not new",e))?;
    let result=(||->Result<(),String>{
        output.write_all(&bytes).map_err(|e|disk_error("Recovery export write failed",e))?;
        output.sync_all().map_err(|e|disk_error("Recovery export flush failed",e))?;
        Ok(())
    })();
    drop(output);
    if result.is_err() {
        let _=fs::remove_file(destination); // only our new, failed export
    }
    result
}

pub fn export_native_ole_backup(workspace:&Path,destination:&Path)->Result<(),String>{
    if !workspace.is_absolute() || !destination.is_absolute() {
        return Err("Recovery export requires absolute Workspace and destination paths".into());
    }
    let backup=backup_path(workspace);
    if destination==workspace || destination==backup {
        return Err("Recovery export must be a distinct new file".into());
    }
    copy_validated_recovery_candidate(&backup,destination)
}

/// Export ONE unambiguous, valid interrupted OLE stage. Never restore it
/// over the current Workspace, and never delete stage/backup. If multiple
/// pending stages exist, refuse so a human can examine each candidate.
pub fn export_native_ole_stage(workspace:&Path,destination:&Path)->Result<(),String>{
    if !workspace.is_absolute() || !destination.is_absolute(){
        return Err("Stage export requires absolute Workspace and destination paths".into());
    }
    let stages=unresolved_workspace_ole_stages(workspace)?;
    if stages.len()!=1 {
        return Err(format!("OLE staged recovery requires exactly one candidate, found {}",stages.len()));
    }
    let stage=&stages[0];
    if destination==workspace || destination==backup_path(workspace) || destination==stage {
        return Err("OLE stage export must target a fresh, separate file".into());
    }
    copy_validated_recovery_candidate(stage,destination)
}


/// Create a user-owned, byte-exact Workspace checkpoint without modifying
/// the live Workspace, Explorer icons or any existing recovery artifacts.
/// The destination MUST be a new file, and a cooperative Workspace lease
/// ensures OLE commits and normal State::save cannot race with this copy.
/// On success the checkpoint is retained for an explicit future rollback.
pub fn create_verified_workspace_safety_checkpoint(
    workspace:&Path,destination:&Path,
)->Result<usize,String>{
    if !workspace.is_absolute() || !destination.is_absolute(){
        return Err("Workspace safety checkpoint requires absolute paths".into());
    }
    let workspace=workspace.canonicalize()
        .map_err(|e|disk_error("Cannot resolve existing Workspace",e))?;
    if !workspace.is_file(){
        return Err("Safety checkpoint Workspace is not an existing file".into());
    }
    let parent=destination.parent().ok_or("Checkpoint destination has no directory")?
        .canonicalize().map_err(|e|disk_error("Cannot resolve checkpoint directory",e))?;
    let leaf=destination.file_name().ok_or("Checkpoint has no name")?;
    let destination=parent.join(leaf);
    if destination==workspace || destination==backup_path(&workspace)
        || destination==lock_path(&workspace)
        || destination==workspace.with_extension("rollback-after"){
        return Err("Checkpoint cannot replace the live Workspace or recovery artifact".into());
    }
    let _lease=WorkspaceLease::acquire(&workspace)?;
    if backup_path(&workspace).exists()
        || workspace.with_extension("native-target-backup").exists()
        || workspace.with_extension("rollback-after").exists()
        || !unresolved_workspace_ole_stages(&workspace)?.is_empty(){
        return Err("Existing recovery artifacts require review before safety checkpoint".into());
    }
    let before=read_existing(&workspace)?;
    State::from_saved_bytes(&before)?;
    let mut file=OpenOptions::new().create_new(true).write(true)
        .open(&destination).map_err(|e|disk_error("Safety checkpoint must be a NEW file",e))?;
    let outcome=(||->Result<(),String>{
        file.write_all(&before).map_err(|e|disk_error("Cannot write Workspace checkpoint",e))?;
        file.sync_all().map_err(|e|disk_error("Cannot flush Workspace checkpoint",e))?;
        drop(file);
        if read_existing(&destination)?!=before {
            return Err("Safety checkpoint bytes do not match captured Workspace".into());
        }
        if read_existing(&workspace)?!=before {
            return Err("Workspace changed while the safety checkpoint was created".into());
        }
        Ok(())
    })();
    if outcome.is_err(){
        let _=fs::remove_file(&destination); // only newly created file
    }
    outcome?;
    Ok(before.len())
}

/// Explicit, conditional rollback of an earlier verified Workspace
/// checkpoint. The current Workspace must be byte-identical to a separately
/// captured expected-post-commit snapshot; otherwise *refuse* rather than
/// clobber subsequent user edits. All existing backups/stages remain untouched.
/// ReplaceFileW retains the exact overwritten predecessor as .rollback-after.
pub fn rollback_workspace_safety_checkpoint(
    workspace:&Path,checkpoint:&Path,expected_current:&Path,
)->Result<usize,String>{
    if !workspace.is_absolute() || !checkpoint.is_absolute()
        || !expected_current.is_absolute(){
        return Err("Rollback requires three absolute file paths".into());
    }
    let workspace=workspace.canonicalize()
        .map_err(|e|disk_error("Cannot resolve live Workspace for rollback",e))?;
    let checkpoint=checkpoint.canonicalize()
        .map_err(|e|disk_error("Cannot resolve checkpoint for rollback",e))?;
    let expected_current=expected_current.canonicalize()
        .map_err(|e|disk_error("Cannot resolve expected current snapshot",e))?;
    if workspace==checkpoint || workspace==expected_current
        || checkpoint==expected_current
        || checkpoint==backup_path(&workspace)
        || expected_current==backup_path(&workspace){
        return Err("Rollback sources must be three distinct, non-recovery files".into());
    }
    let original=read_existing(&checkpoint)?;
    let expected=read_existing(&expected_current)?;
    State::from_saved_bytes(&original)?;
    State::from_saved_bytes(&expected)?;
    if original==expected {
        return Err("Rollback requires a distinct verified post-commit Workspace".into());
    }
    let _lease=WorkspaceLease::acquire(&workspace)?;
    let preserved=workspace.with_extension("rollback-after");
    if backup_path(&workspace).exists() || preserved.exists()
        || workspace.with_extension("native-target-backup").exists()
        || !unresolved_workspace_ole_stages(&workspace)?.is_empty(){
        return Err("Rollback refused: unresolved recovery artifact already exists".into());
    }
    if read_existing(&workspace)?!=expected {
        return Err("Rollback refused: live Workspace differs from expected post-commit snapshot".into());
    }
    let mut stage=StageFile::new(&workspace)?;
    stage.stage(&original)?;
    if read_existing(&workspace)?!=expected{
        return Err("Rollback refused: Workspace was edited during staging".into());
    }
    replace_preserving_backup(&workspace,&stage.path,&preserved)?;
    verify_replaced_predecessor(&preserved,&expected)?;
    if read_existing(&workspace)?!=original{
        return Err("Rollback postflight failed; retained overwritten Workspace for manual recovery".into());
    }
    // Never delete the preserved post-commit image automatically: it is the
    // user's escape hatch if a rollback itself was the wrong decision.
    Ok(original.len())
}


/// Explicit native target creation is a different operation from an OLE drag.
/// It is deliberately not called during normal app startup. The caller must
/// verify the physical Explorer icon layout and the native two-Box plan.
/// The active Workspace is replaced conditionally, with its prior image
/// both in the user checkpoint AND a crash-preserved Windows backup until
/// all postflight comparisons pass.
pub(crate) fn commit_checked_native_destination(
    workspace:&Path,checkpoint:&Path,expected_post:&Path,
    before:&State,after:&State,
)->Result<usize,String>{
    if !workspace.is_absolute() || !checkpoint.is_absolute()
        || !expected_post.is_absolute(){
        return Err("Native destination commit requires absolute file paths".into());
    }
    let workspace=workspace.canonicalize()
        .map_err(|e|disk_error("Native destination Workspace unavailable",e))?;
    let checkpoint=checkpoint.canonicalize()
        .map_err(|e|disk_error("Native destination checkpoint unavailable",e))?;
    let parent=expected_post.parent()
        .ok_or("Expected Native destination post-image has no parent")?
        .canonicalize().map_err(|e|disk_error("Expected post-image directory unavailable",e))?;
    let expected_post=parent.join(
        expected_post.file_name().ok_or("Expected post-image has no filename")?);
    let rollback=workspace.with_extension("rollback-after");
    let replacement_backup=workspace.with_extension("native-target-backup");
    if checkpoint==workspace || checkpoint==expected_post ||
        expected_post==workspace || expected_post==replacement_backup ||
        expected_post==rollback || checkpoint==rollback ||
        checkpoint==replacement_backup {
        return Err("Native destination checkpoint/post-image must be distinct from live recovery files".into());
    }
    let before_json=serde_json::to_value(before).map_err(|e|e.to_string())?;
    let after_json=serde_json::to_value(after).map_err(|e|e.to_string())?;
    let (mut previous,mut candidate)=(before_json,after_json);
    let src_groups=previous.as_object_mut().ok_or("Invalid original Workspace JSON object")?
        .remove("groups").ok_or("Missing original groups")?;
    let dest_groups=candidate.as_object_mut().ok_or("Invalid destination Workspace JSON object")?
        .remove("groups").ok_or("Missing destination groups")?;
    previous.as_object_mut().unwrap().remove("next_id");
    candidate.as_object_mut().unwrap().remove("next_id");
    if previous!=candidate || before.groups.len()+1!=after.groups.len()
        || after.next_id!=before.next_id.checked_add(3)
            .ok_or("Native destination Group ID overflow")? {
        return Err("Native destination must append exactly one Box without modifying other user settings".into());
    }
    let src_arr=src_groups.as_array().ok_or("Malformed original Box list")?;
    let dest_arr=dest_groups.as_array().ok_or("Malformed new Box list")?;
    if dest_arr.get(..src_arr.len())!=Some(src_arr.as_slice()){
        return Err("Native destination modified or reordered an existing Box".into());
    }
    let appended=after.groups.last().ok_or("New destination Box missing")?;
    if appended.page!=before.page || appended.rolled || appended.tabs.len()!=2 ||
        appended.active!=0 || appended.tabs.iter().any(|t|
            t.folder.is_some() || !t.items.is_empty()) {
        return Err("Native destination must be an active empty regular two-Tab Box".into());
    }
    let _lease=WorkspaceLease::acquire(&workspace)?;
    if backup_path(&workspace).exists() || replacement_backup.exists()
        || rollback.exists()
        || !unresolved_workspace_ole_stages(&workspace)?.is_empty(){
        return Err("Native destination refused due to pending Workspace recovery artifact".into());
    }
    let original=read_existing(&workspace)?;
    if read_existing(&checkpoint)?!=original {
        return Err("Native destination refused: safety checkpoint differs from live Workspace".into());
    }
    let loaded=State::from_saved_bytes(&original)?;
    if serialized_state(&loaded)?!=serialized_state(before)?{
        return Err("Native destination refused: saved Workspace State differs from preflight".into());
    }
    let new_bytes=serde_json::to_vec_pretty(after).map_err(|e|e.to_string())?;
    if new_bytes.len()>MAX_WORKSPACE_BYTES{
        return Err("Native destination post-image too large".into());
    }
    let mut file=OpenOptions::new().create_new(true).write(true)
        .open(&expected_post).map_err(|e|disk_error("Expected post-image must be NEW",e))?;
    file.write_all(&new_bytes).map_err(|e|disk_error("Cannot stage expected post-image",e))?;
    file.sync_all().map_err(|e|disk_error("Cannot fsync expected post-image",e))?;
    drop(file);
    if read_existing(&expected_post)?!=new_bytes{
        return Err("Expected Native destination post-image is not byte-exact".into());
    }
    let mut stage=StageFile::new(&workspace)?;
    stage.stage(&new_bytes)?;
    if read_existing(&workspace)?!=original{
        return Err("Native destination refused: live Workspace changed before ReplaceFileW".into());
    }
    replace_preserving_backup(&workspace,&stage.path,&replacement_backup)?;
    verify_replaced_predecessor(&replacement_backup,&original)?;
    if read_existing(&workspace)?!=new_bytes{
        return Err("Native destination postflight mismatch; recovery predecessor retained".into());
    }
    if read_existing(&checkpoint)?!=original ||
        read_existing(&expected_post)?!=new_bytes{
        return Err("Native destination backup or post-image changed after commit".into());
    }
    // The independent checkpoint persists in the user-selected archive.
    // A failed removal leaves the Windows predecessor as an explicit
    // recovery artifact; State::save refuses normal writes while it exists.
    if let Err(e)=fs::remove_file(&replacement_backup){
        return Err(format!(
            "Native destination saved, but transient predecessor could not be cleared; no further saves allowed until reviewed: {e}"));
    }
    Ok(new_bytes.len())
}

/// Exactly one consumed, conditional, on-disk transaction.
/// All arguments are copied from a future caller's verified UI gesture.
pub struct NativeOlePersistedGesture {
    ticket: NativeOleDropTicket,
    workspace: PathBuf,
    expected_disk: Vec<u8>,
}

/// Lifecycle of an eventual OLE receiver with disk persistence. The data
/// object is decoded by the caller; this core does not create a window,
/// install a hook or interact with Explorer.
#[derive(Default)]
pub struct NativeOlePersistedSession {
    pending: Option<NativeOlePersistedGesture>,
}
impl NativeOlePersistedSession {
    pub fn drag_enter(
        &mut self,
        workspace: &Path,
        live: &State,
        explorer: &[ViewItem],
        monitor_signature: &str,
        viewport: NativeOleViewport,
        offered_paths: &[PathBuf],
        visible_boxes: &[u64],
        max_groups: usize,
        max_items_per_tab: usize,
    ) -> Result<(), String> {
        // A new OLE gesture must not inherit paths from the previous one.
        self.pending.take();
        self.pending = Some(NativeOlePersistedGesture::begin(
            workspace,
            live,
            explorer,
            monitor_signature,
            viewport,
            offered_paths,
            visible_boxes,
            max_groups,
            max_items_per_tab,
        )?);
        Ok(())
    }

    /// Return LINK only for valid candidate locations and only when the
    /// source offers LINK. Workspace disk race is checked again at Drop, so
    /// DragOver never promises that the eventual filesystem commit succeeds.
    pub fn drag_over_effect(
        &self,
        live: &State,
        explorer: &[ViewItem],
        monitor_signature: &str,
        viewport: NativeOleViewport,
        visible_boxes: &[u64],
        received_paths: &[PathBuf],
        screen: (i32, i32),
        source_effects: u32,
    ) -> u32 {
        if source_effects & 4 == 0 {
            return 0;
        }
        if self.pending.as_ref().is_some_and(|gesture| {
            gesture
                .preview(
                    live,
                    explorer,
                    monitor_signature,
                    viewport,
                    visible_boxes,
                    received_paths,
                    screen,
                )
                .is_ok()
        }) {
            4
        } else {
            0
        }
    }

    pub fn drag_leave(&mut self) {
        self.pending.take();
    }

    /// Consume the staged gesture on every outcome. The caller's native COM
    /// Drop callback must set DROPEFFECT_LINK only after this returns Ok.
    /// Err means DROPEFFECT_NONE, even if DragOver previously offered LINK.
    pub fn drop(
        &mut self,
        live: &mut State,
        explorer: &[ViewItem],
        monitor_signature: &str,
        viewport: NativeOleViewport,
        visible_boxes: &[u64],
        received_paths: &[PathBuf],
        screen: (i32, i32),
        source_effects: u32,
    ) -> Result<NativeMultiBoxPlan, String> {
        let gesture = self
            .pending
            .take()
            .ok_or("OLE persisted Drop has no current gesture")?;
        gesture.commit(
            live,
            explorer,
            monitor_signature,
            viewport,
            visible_boxes,
            received_paths,
            screen,
            source_effects,
        )
    }
}
impl NativeOlePersistedGesture {
    pub fn begin(
        workspace: &Path,
        state: &State,
        explorer: &[ViewItem],
        monitor_signature: &str,
        viewport: NativeOleViewport,
        offered_paths: &[PathBuf],
        visible_box_ids: &[u64],
        max_groups: usize,
        max_items_per_tab: usize,
    ) -> Result<Self, String> {
        if !workspace.is_absolute() {
            return Err("Persisted OLE workspace path must be absolute".into());
        }
        let initial = read_existing(workspace)?;
        let loaded = State::load(workspace)?;
        if serialized_state(&loaded)? != serialized_state(state)? {
            return Err("In-memory Workspace differs from its saved version".into());
        }
        let ticket = NativeOleDropTicket::begin(
            state,
            explorer,
            monitor_signature,
            viewport,
            offered_paths,
            visible_box_ids,
            max_groups,
            max_items_per_tab,
        )?;
        // File could have been replaced during policy planning.
        if read_existing(workspace)? != initial {
            return Err("Workspace changed during OLE DragEnter".into());
        }
        Ok(Self {
            ticket,
            workspace: workspace.to_path_buf(),
            expected_disk: initial,
        })
    }

    /// Non-mutating OLE hover validation. It intentionally does not read or
    /// lock the Workspace on each mouse-move event; the commit always does
    /// the complete disk revalidation.
    pub fn preview(
        &self,
        live: &State,
        explorer: &[ViewItem],
        monitor_signature: &str,
        viewport: NativeOleViewport,
        visible_boxes: &[u64],
        received_paths: &[PathBuf],
        screen: (i32, i32),
    ) -> Result<(u64, u64), String> {
        self.ticket.preview(
            live,
            explorer,
            monitor_signature,
            viewport,
            visible_boxes,
            received_paths,
            screen,
        )
    }

    /// Persist first, publish the new in-memory State only after verification.
    /// On all validation or pre-replace failures, neither State nor target
    /// Workspace is modified. A backup from any interrupted transaction
    /// blocks future commits until reviewed/recovered manually.
    pub fn commit(
        self,
        live: &mut State,
        explorer: &[ViewItem],
        monitor_signature: &str,
        viewport: NativeOleViewport,
        live_visible_box_ids: &[u64],
        received_paths: &[PathBuf],
        screen: (i32, i32),
        effects_offered_by_source: u32,
    ) -> Result<NativeMultiBoxPlan, String> {
        if effects_offered_by_source & 4 == 0 {
            return Err("OLE persisted source did not offer LINK".into());
        }
        self.commit_virtual_pin_authorized(
            live,explorer,monitor_signature,viewport,live_visible_box_ids,
            received_paths,screen)
    }

    /// The explicit DEBUG in-process Explorer MouseDown/MouseUp lab has NO
    /// IDataObject and must NOT pretend Explorer offered DROPEFFECT_LINK.
    /// This entry point exists only in the unsafe native lab build; its
    /// caller requires a verified one-shot, worker-produced hook event and
    /// an exact cloned OS TEMP Workspace. Never enable in a normal binary.
    #[cfg(all(windows,feature="native-unsafe-drag-lab"))]
    pub(crate) fn commit_verified_native_gesture_lab(
        self,
        live:&mut State,
        explorer:&[ViewItem],
        monitor_signature:&str,
        viewport:NativeOleViewport,
        live_visible_box_ids:&[u64],
        received_paths:&[PathBuf],
        screen:(i32,i32),
        prepared:&crate::native_drag_channel::PreparedNativeGesture,
    )->Result<NativeMultiBoxPlan,String>{
        if received_paths!=prepared.offered_paths ||
           !live_visible_box_ids.contains(&prepared.target_group) ||
           prepared.target_group==0 || prepared.target_tab==0 {
            return Err("Real native gesture permit does not match offered paths or target".into());
        }
        self.commit_virtual_pin_authorized(
            live,explorer,monitor_signature,viewport,live_visible_box_ids,
            received_paths,screen)
    }

    fn commit_virtual_pin_authorized(
        self,
        live:&mut State,
        explorer:&[ViewItem],
        monitor_signature:&str,
        viewport:NativeOleViewport,
        live_visible_box_ids:&[u64],
        received_paths:&[PathBuf],
        screen:(i32,i32),
    )->Result<NativeMultiBoxPlan,String>{
        let mut next = live.clone();
        let plan = self.ticket.commit(
            &mut next,
            explorer,
            monitor_signature,
            viewport,
            live_visible_box_ids,
            received_paths,
            screen,
        )?;
        // All of the following occurs without changing the live UI state.
        let _lease = WorkspaceLease::acquire(&self.workspace)?;
        let backup = backup_path(&self.workspace);
        if backup.exists()
            || self.workspace.with_extension("native-target-backup").exists()
            || self.workspace.with_extension("rollback-after").exists() {
            return Err("An unresolved Workspace recovery predecessor exists; review before OLE retry".into());
        }
        if !unresolved_workspace_ole_stages(&self.workspace)?.is_empty() {
            return Err("Unresolved OLE staged transaction exists; review before retry".into());
        }
        if read_existing(&self.workspace)? != self.expected_disk {
            return Err("Workspace was edited externally during the OLE gesture".into());
        }
        let disk = State::load(&self.workspace)?;
        if serialized_state(&disk)? != serialized_state(live)? {
            return Err("Workspace changed or was not persisted before the OLE drop".into());
        }
        let next_bytes = serde_json::to_vec_pretty(&next)
            .map_err(|e| format!("Could not serialize OLE Workspace: {e}"))?;
        let mut stage = StageFile::new(&self.workspace)?;
        stage.stage(&next_bytes)?;
        // Cooperative sidecar blocks other OLE operations; still recheck the
        // current file before replacement. Old uncooperative State::save
        // callers need integration before a production release.
        if read_existing(&self.workspace)? != self.expected_disk {
            return Err("Workspace changed before conditional OLE replacement".into());
        }
        replace_preserving_backup(&self.workspace, &stage.path, &backup)?;
        // A writer which ignores our cooperative sidecar may still replace
        // the workspace after the final byte comparison. ReplaceFileW's
        // predecessor backup exposes that race; fail closed and retain it.
        verify_replaced_predecessor(&backup,&self.expected_disk)?;
        let check = read_existing(&self.workspace);
        if check.as_deref() != Ok(next_bytes.as_slice()) {
            // Keep backup rather than claiming success or risking a blind
            // second replacement after an incomplete postflight.
            return Err("OLE persisted postflight mismatch: backup retained for recovery".into());
        }
        *live = next;
        // Once the replacement is verified, unlink the recovery copy if
        // possible. Failure here cannot turn a successful commit into a
        // failure response, because the file and UI now both contain "next".
        // A retained backup blocks future OLE commits pending review.
        let _ = fs::remove_file(&backup);
        Ok(plan)
    }
}


pub(crate) const MAX_NATIVE_PIN_PATHS: usize = crate::native_filter_protocol::MAX_ITEMS;

/// Acknowledged *virtual* native drags may be committed to the real
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NativePinDeltaMode {
    Drag,
    SortRules,
    RestoreLayout,
}

/// Workspace, but only as a pure membership delta for those paths. Every other
/// Box, Tab, setting, rule, Page and Pin must remain byte-equivalent.
fn native_pin_only_delta(before:&State,after:&State,path:&str)->Result<(),String>{
    validate_native_pin_delta(before, after, &[path.to_string()], NativePinDeltaMode::Drag)
}

fn validate_native_pin_delta(
    before:&State,after:&State,paths:&[String],mode:NativePinDeltaMode,
)->Result<(),String>{
    if paths.is_empty() || paths.len() > MAX_NATIVE_PIN_PATHS || before.groups.len()!=after.groups.len() {
        return Err("Native Pin transaction must preserve all Box structures".into());
    }
    let mut path_keys = std::collections::BTreeSet::new();
    for path in paths {
        let trimmed = path.trim();
        if trimmed.is_empty() || !path_keys.insert(trimmed.to_lowercase()) {
            return Err("Native Pin transaction requires distinct non-empty paths".into());
        }
    }
    let mut old=serde_json::to_value(before).map_err(|e|e.to_string())?;
    let mut new=serde_json::to_value(after).map_err(|e|e.to_string())?;
    let mut owners: [std::collections::BTreeMap<String, Vec<(u64, u64, usize)>>; 2] = Default::default();
    let mut pin_values: [std::collections::BTreeMap<String, Vec<serde_json::Value>>; 2] = Default::default();
    for (slot,state) in [&mut old,&mut new].into_iter().enumerate() {
        let groups=state.get_mut("groups")
            .and_then(serde_json::Value::as_array_mut)
            .ok_or("Native Pin transaction lacks Box array")?;
        for group in groups {
            let gid=group.get("id").and_then(serde_json::Value::as_u64)
                .ok_or("Native Box lacks its stable ID")?;
            let tabs=group.get_mut("tabs")
                .and_then(serde_json::Value::as_array_mut)
                .ok_or("Native Box lacks its Tabs")?;
            for tab in tabs{
                let tid=tab.get("id").and_then(serde_json::Value::as_u64)
                    .ok_or("Native Tab lacks its stable ID")?;
                let portal=tab.get("folder").and_then(serde_json::Value::as_str)
                    .is_some();
                let items=tab.get_mut("items")
                    .and_then(serde_json::Value::as_array_mut)
                    .ok_or("Native Tab lacks its Pin list")?;
                if portal {continue;} // a Portal's contents NEVER change here
                let mut rest=Vec::new();
                for (index, item) in items.drain(..).enumerate(){
                    let matched_key = item.get("path")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_lowercase)
                        .filter(|k| path_keys.contains(k));
                    if let Some(key) = matched_key {
                        owners[slot].entry(key.clone()).or_default().push((gid, tid, index));
                        pin_values[slot].entry(key).or_default().push(item);
                    }else{rest.push(item);}
                }
                *items=rest;
            }
        }
        if let Some(obj)=state.as_object_mut(){obj.remove("next_id");}
    }
    if old != new {
        return Err("Native Pin Drop changes unrelated Workspace state".into());
    }
    let mut from_loose_count = 0u64;
    let mut new_ids = Vec::new();
    let mut existing_pin_count = 0usize;
    let mut bulk_ownership = None;
    let mut changed = false;
    for key in &path_keys {
        let o_before = owners[0].get(key).map(Vec::as_slice).unwrap_or(&[]);
        let o_after = owners[1].get(key).map(Vec::as_slice).unwrap_or(&[]);
        if o_before.len() > 1 || o_after.len() > 1
            || (o_before.is_empty() && o_after.is_empty())
        {
            return Err("Native Pin Drop does not move a unique virtual Pin per selected item".into());
        }
        changed |= o_before != o_after;
        if mode == NativePinDeltaMode::SortRules && o_after.is_empty() {
            return Err("Sorting by Rules may only assign or move an icon into a regular Tab".into());
        }
        if mode == NativePinDeltaMode::Drag && paths.len() > 8 {
            let ownership = (o_before.first().map(|&(g, t, _)| (g, t)),
                o_after.first().map(|&(g, t, _)| (g, t)));
            if bulk_ownership.replace(ownership).is_some_and(|prior| prior != ownership) {
                return Err("Large Native Pin transaction requires the same source and destination for the selection".into());
            }
        }
        if o_before.is_empty() {
            from_loose_count += 1;
            let assigned_id = pin_values[1].get(key)
                .and_then(|v| v.first())
                .and_then(|v| v.get("id"))
                .and_then(serde_json::Value::as_u64)
                .ok_or("Native loose item Pin requires a stable Item ID")?;
            new_ids.push(assigned_id);
        } else {
            existing_pin_count += 1;
            if let (Some(old_val), Some(new_val)) = (
                pin_values[0].get(key).and_then(|v| v.first()),
                pin_values[1].get(key).and_then(|v| v.first()),
            ) {
                if old_val != new_val {
                    return Err("Native transfer must preserve the complete original virtual Pin identity and metadata".into());
                }
            }
        }
    }
    if !changed {
        return Err("Native Pin transaction does not change placement order or ownership".into());
    }
    if mode == NativePinDeltaMode::Drag && from_loose_count > 0 && existing_pin_count > 0 {
        return Err("Native Pin transaction cannot mix Box and loose Desktop items".into());
    }
    if from_loose_count > 0 {
        new_ids.sort_unstable();
        let expected_ids: Vec<u64> = (before.next_id..before.next_id + from_loose_count).collect();
        if after.next_id != before.next_id + from_loose_count || new_ids != expected_ids {
            return Err("Native loose item Pins require sequential stable Item IDs".into());
        }
    } else if before.next_id != after.next_id {
        return Err("Moving or unpinning existing Pins may not allocate IDs".into());
    }
    Ok(())
}

/// Write only the verified source/target regular-Tab item arrays into the
/// original JSON tree. Do not materialize optional preference defaults or
/// drop unrelated unknown forward-compatible fields when serializing State.
fn patch_only_native_pins_in_saved_json(
    original_bytes:&[u8],
    saved_target:&State,
    moved_paths:&[String],
)->Result<Vec<u8>,String>{
    let keys = moved_paths.iter().map(|p| p.to_lowercase()).collect::<std::collections::BTreeSet<_>>();
    let mut raw:serde_json::Value=serde_json::from_slice(original_bytes)
        .map_err(|e|format!("Persisted Workspace JSON invalid: {e}"))?;
    let groups=raw.get_mut("groups")
        .and_then(serde_json::Value::as_array_mut)
        .ok_or("Persisted Workspace lacks native groups array")?;
    let mut source_items: std::collections::BTreeMap<String, serde_json::Value> = Default::default();
    for group in groups.iter_mut(){
        let tabs=group.get_mut("tabs")
            .and_then(serde_json::Value::as_array_mut)
            .ok_or("Persisted Group lacks Tabs")?;
        for tab in tabs {
            if tab.get("folder").is_some_and(|v|!v.is_null()){continue;}
            let items=tab.get_mut("items")
                .and_then(serde_json::Value::as_array_mut)
                .ok_or("Persisted regular Tab lacks items")?;
            let mut index=0;
            while index<items.len(){
                let matched=items[index].get("path")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_lowercase)
                    .filter(|k| keys.contains(k));
                if let Some(key) = matched {
                    if source_items.insert(key, items.remove(index)).is_some() {
                        return Err("Workspace contains duplicated native source Pin identity".into());
                    }
                }else{index+=1;}
            }
        }
    }
    let mut destinations: std::collections::BTreeMap<String, (u64, u64, usize, &crate::model::Item)> = Default::default();
    for group in &saved_target.groups {
        for tab in &group.tabs {
            if tab.folder.is_some(){continue;}
            for (index,pin) in tab.items.iter().enumerate(){
                let key = pin.path.to_lowercase();
                if keys.contains(&key){
                    if destinations.insert(key, (group.id,tab.id,index,pin)).is_some(){
                        return Err("Proposed native Pin is duplicated across regular Tabs".into());
                    }
                }
            }
        }
    }
    for key in &keys {
        if !source_items.contains_key(key) && !destinations.contains_key(key) {
            return Err("Native Pin source and destination are both absent".into());
        }
    }
    let mut inserts = destinations.into_iter().collect::<Vec<_>>();
    inserts.sort_by_key(|(_, (gid, tid, at, _))| (*gid, *tid, *at));
    for (key, (group_id,tab_id,at,pin)) in inserts {
        let value=if let Some(original)=source_items.remove(&key){
            original
        }else{
            serde_json::to_value(pin)
                .map_err(|e|format!("New native Pin metadata invalid: {e}"))?
        };
        let group=groups.iter_mut().find(|g|g.get("id")
            .and_then(serde_json::Value::as_u64)==Some(group_id))
            .ok_or("Committed native destination Box missing in original JSON")?;
        let tab=group.get_mut("tabs").and_then(serde_json::Value::as_array_mut)
            .and_then(|tabs|tabs.iter_mut().find(|t|t.get("id")
                .and_then(serde_json::Value::as_u64)==Some(tab_id)))
            .ok_or("Committed native destination Tab missing in original JSON")?;
        if tab.get("folder").is_some_and(|v|!v.is_null()){
            return Err("Native Pin would enter a linked-folder Portal".into());
        }
        let items=tab.get_mut("items")
            .and_then(serde_json::Value::as_array_mut)
            .ok_or("Committed native target items missing")?;
        if at>items.len(){
            return Err("Native destination index no longer matches original Workspace".into());
        }
        items.insert(at,value);
    }
    let original:State=State::from_saved_bytes(original_bytes)?;
    if original.next_id!=saved_target.next_id {
        let root=raw.as_object_mut().ok_or("Workspace JSON root not an object")?;
        root.insert("next_id".into(),
            serde_json::Value::from(saved_target.next_id));
    }
    let loaded:State=serde_json::from_value(raw.clone())
        .map_err(|e|format!("Native JSON patch produced unreadable Workspace: {e}"))?;
    if serialized_state(&loaded)?!=serialized_state(saved_target)?{
        return Err("Native JSON membership patch changed or lost unrelated Workspace state".into());
    }
    serde_json::to_vec_pretty(&raw)
        .map_err(|e|format!("Native JSON patch serialization failed: {e}"))
}

/// Crash-safe and conditional persistence AFTER the Explorer worker has
/// acknowledged the new icon layout. If a process dies between atomic
/// ReplaceFileW and in-memory handoff, an exact before-image backup remains
/// for explicit review. The native-position journal is a separate concern.
pub(crate) fn commit_verified_native_pin_drop(
    workspace:&Path,before:&State,after:&State,source_path:&str,
)->Result<usize,String>{
    commit_verified_native_pin_drop_over(workspace,before,before,after,source_path)
}

/// `persisted` with only the Pins for `paths` moved to where `after` holds them.
pub(crate) fn pins_change_onto(persisted:&State,after:&State,paths:&[String])->Result<State,String>{
    let keys = paths.iter().map(|p| p.to_lowercase()).collect::<std::collections::BTreeSet<_>>();
    let mut found: Vec<(u64, u64, usize, crate::model::Item)> = Vec::new();
    let mut seen_keys = std::collections::BTreeSet::new();
    for group in &after.groups {
        for tab in group.tabs.iter().filter(|tab|tab.folder.is_none()) {
            for (index,pin) in tab.items.iter().enumerate() {
                let key = pin.path.to_lowercase();
                if keys.contains(&key) {
                    if !seen_keys.insert(key) {
                        return Err("Proposed native Pin is duplicated across regular Tabs".into());
                    }
                    found.push((group.id,tab.id,index,pin.clone()));
                }
            }
        }
    }
    found.sort_by_key(|(gid, tid, index, _)| (*gid, *tid, *index));
    let mut next=persisted.clone();
    for group in &mut next.groups {
        for tab in group.tabs.iter_mut().filter(|tab|tab.folder.is_none()) {
            tab.items.retain(|pin|!keys.contains(&pin.path.to_lowercase()));
        }
    }
    for (group_id,tab_id,index,pin) in found {
        let tab=next.groups.iter_mut().find(|group|group.id==group_id)
            .and_then(|group|group.tabs.iter_mut().find(|tab|tab.id==tab_id))
            .filter(|tab|tab.folder.is_none())
            .ok_or("Native Pin destination Tab is not in the saved Workspace")?;
        tab.items.insert(index.min(tab.items.len()),pin);
    }
    next.next_id=next.next_id.max(after.next_id);
    Ok(next)
}

pub(crate) fn commit_verified_native_pin_drop_over(
    workspace:&Path,persisted:&State,before:&State,after:&State,source_path:&str,
)->Result<usize,String>{
    commit_verified_native_pin_drops_over(workspace, persisted, before, after, &[source_path.to_string()])
}

/// Native Desktop keeps Tab switches and Box moves in memory only, so the
/// accepted State (`before`) may differ from the file. `persisted` is the
/// State this session last wrote or loaded: the file must still match it
/// (no other writer), while `before`→`after` must be this atomic Pin change.
/// Only those Pins are patched into the file; nothing else unsaved is written.
pub(crate) fn commit_verified_native_pin_drops_over(
    workspace:&Path,persisted:&State,before:&State,after:&State,source_paths:&[String],
)->Result<usize,String>{
    commit_native_pin_delta_over(workspace, persisted, before, after, source_paths, NativePinDeltaMode::Drag)
}

pub(crate) fn commit_verified_native_rules_sort_over(
    workspace:&Path,persisted:&State,before:&State,after:&State,source_paths:&[String],
)->Result<usize,String>{
    commit_native_pin_delta_over(workspace, persisted, before, after, source_paths, NativePinDeltaMode::SortRules)
}

pub(crate) fn commit_verified_native_layout_restore_over(
    workspace:&Path,persisted:&State,before:&State,after:&State,source_paths:&[String],
)->Result<usize,String>{
    commit_native_pin_delta_over(workspace, persisted, before, after, source_paths, NativePinDeltaMode::RestoreLayout)
}

fn commit_native_pin_delta_over(
    workspace:&Path,persisted:&State,before:&State,after:&State,source_paths:&[String],mode:NativePinDeltaMode,
)->Result<usize,String>{
    commit_native_pin_delta_over_bytes(workspace,persisted,before,after,source_paths,mode,None)
        .map(|(_,new_bytes)|new_bytes.len())
}

fn commit_native_pin_delta_over_bytes(
    workspace:&Path,persisted:&State,before:&State,after:&State,source_paths:&[String],mode:NativePinDeltaMode,
    expected_predecessor: Option<&[u8]>,
)->Result<(Vec<u8>,Vec<u8>),String>{
    if !workspace.is_absolute() || !workspace.is_file(){
        return Err("Real native Pin commit requires existing absolute Workspace".into());
    }
    validate_native_pin_delta(before,after,source_paths,mode)?;
    // What the file should hold afterwards: the persisted State plus these Pins.
    let disk_after=if serialized_state(persisted)?==serialized_state(before)? {
        after.clone()
    }else{
        pins_change_onto(persisted,after,source_paths)?
    };
    validate_native_pin_delta(persisted,&disk_after,source_paths,mode)?;
    let _lease=WorkspaceLease::acquire(workspace)?;
    let recovery=backup_path(workspace);
    if recovery.exists() ||
       workspace.with_extension("rollback-after").exists() ||
       workspace.with_extension("native-target-backup").exists() ||
       !unresolved_workspace_ole_stages(workspace)?.is_empty(){
        return Err("A previous Workspace recovery image exists; refuse native Pin replacement".into());
    }
    let predecessor=read_existing(workspace)?;
    if let Some(expected) = expected_predecessor {
        if predecessor.as_slice() != expected {
            return Err("Workspace file changed externally during AI transaction; refusing to overwrite newer workspace".into());
        }
    }
    let loaded=State::from_saved_bytes(&predecessor)?;
    if serialized_state(&loaded)?!=serialized_state(persisted)? {
        return Err("Disk Workspace differs from native accepted State".into());
    }
    let new_bytes=patch_only_native_pins_in_saved_json(
        &predecessor,&disk_after,source_paths)?;
    if new_bytes.len()>MAX_WORKSPACE_BYTES{
        return Err("Native Pin Workspace would exceed 32 MiB".into());
    }
    let mut stage=StageFile::new(workspace)?;
    stage.stage(&new_bytes)?;
    if read_existing(workspace)?!=predecessor{
        return Err("Concurrent Workspace writer changed original bytes before Native Pin ReplaceFileW".into());
    }
    replace_preserving_backup(workspace,&stage.path,&recovery)?;
    verify_replaced_predecessor(&recovery,&predecessor)?;
    if read_existing(workspace)?!=new_bytes {
        return Err("Native Pin postflight failed; immutable predecessor retained for review".into());
    }
    if let Err(error)=fs::remove_file(&recovery){
        return Err(format!(
            "Native Pin saved but its predecessor backup could not be cleared: {error}; normal saves must remain suspended"));
    }
    Ok((predecessor, new_bytes))
}

pub fn commit_verified_native_layout_restore_over_with_bytes(
    workspace: &Path,
    persisted: &State,
    before: &State,
    after: &State,
    source_paths: &[String],
    expected_predecessor: Option<&[u8]>,
) -> Result<(Vec<u8>, Vec<u8>), String> {
    commit_native_pin_delta_over_bytes(
        workspace,
        persisted,
        before,
        after,
        source_paths,
        NativePinDeltaMode::RestoreLayout,
        expected_predecessor,
    )
}

/// Write new workspace bytes under `WorkspaceLease` while capturing the exact
/// bounded predecessor bytes and requiring the live file to match
/// `expected_current` (baseline bytes on the first AI write, or the previous AI
/// write's exact bytes on subsequent stages).
/// Requires an existing, non-empty workspace file and always uses `StageFile` +
/// `ReplaceFileW` + `verify_replaced_predecessor` so cooperative concurrent
/// writers are refused before replacement and uncooperative writers racing in
/// the final window are detected with their exact image preserved in the
/// recovery backup.
pub(crate) fn replace_verified_workspace_bytes(
    workspace: &Path,
    expected_current: &[u8],
    new_bytes: &[u8],
) -> Result<(Vec<u8>, Vec<u8>), String> {
    if new_bytes.is_empty() || new_bytes.len() > MAX_WORKSPACE_BYTES {
        return Err("Workspace exceeds the readable size limit; previous layout preserved".into());
    }
    let _lease = WorkspaceLease::acquire(workspace)?;
    let recovery = backup_path(workspace);
    if recovery.exists()
        || workspace.with_extension("rollback-after").exists()
        || workspace.with_extension("native-target-backup").exists()
        || !unresolved_workspace_ole_stages(workspace)?.is_empty()
    {
        return Err("Workspace has an unresolved recovery backup or predecessor; inspect before saving".into());
    }
    let predecessor = read_existing(workspace)?;
    if predecessor.as_slice() != expected_current {
        return Err("Workspace file changed externally during AI transaction; refusing to overwrite newer workspace".into());
    }
    let mut stage = StageFile::new(workspace)?;
    stage.stage(new_bytes)?;
    if read_existing(workspace)? != predecessor {
        return Err("Concurrent Workspace writer changed original bytes before ReplaceFileW".into());
    }
    replace_preserving_backup(workspace, &stage.path, &recovery)?;
    verify_replaced_predecessor(&recovery, &predecessor)?;
    if read_existing(workspace)? != new_bytes {
        return Err("Workspace save postflight failed; predecessor backup retained for review".into());
    }
    if let Err(error) = fs::remove_file(&recovery) {
        return Err(format!(
            "Workspace saved but its predecessor backup could not be cleared: {error}; normal saves must remain suspended"
        ));
    }
    Ok((predecessor, new_bytes.to_vec()))
}

/// Conditionally restore exact predecessor bytes under `WorkspaceLease` using
/// predecessor-preserving `ReplaceFileW`. Refuses to replace when the live file
/// differs from `expected_current` under the lease; if an uncooperative external
/// writer modifies the file in the final `ReplaceFileW` window,
/// `verify_replaced_predecessor` detects the race and retains the overwritten
/// bytes in `.ole-backup` with normal saves suspended for manual recovery.
pub fn rollback_verified_workspace_bytes(
    workspace: &Path,
    expected_current: &[u8],
    predecessor: &[u8],
) -> Result<(), String> {
    if predecessor.is_empty() || predecessor.len() > MAX_WORKSPACE_BYTES {
        return Err("Rollback predecessor bytes are invalid".into());
    }
    let _lease = WorkspaceLease::acquire(workspace)?;
    let recovery = backup_path(workspace);
    if recovery.exists()
        || workspace.with_extension("rollback-after").exists()
        || workspace.with_extension("native-target-backup").exists()
        || !unresolved_workspace_ole_stages(workspace)?.is_empty()
    {
        return Err("A previous Workspace recovery image exists; refuse automated rollback".into());
    }
    let current = read_existing(workspace)?;
    if current != expected_current {
        return Err("Workspace file changed externally; refusing to overwrite newer workspace".into());
    }
    let mut stage = StageFile::new(workspace)?;
    stage.stage(predecessor)?;
    if read_existing(workspace)? != expected_current {
        return Err("Concurrent Workspace writer changed bytes before rollback replacement".into());
    }
    replace_preserving_backup(workspace, &stage.path, &recovery)?;
    verify_replaced_predecessor(&recovery, expected_current)?;
    if read_existing(workspace)? != predecessor {
        return Err("Workspace rollback postflight failed; backup retained for review".into());
    }
    if let Err(error) = fs::remove_file(&recovery) {
        return Err(format!(
            "Workspace rolled back but its backup could not be cleared: {error}"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod native_pin_atomic_commit_tests {
    use super::*;
    use crate::model::{Bounds,Item};
    fn pin(path:&str,id:u64)->Item{
        Item{id,name:"Shortcut".into(),path:path.into(),kind:"lnk".into(),
            size:1,modified:0,label:None}
    }
    fn seed()->(State,u64,u64,String){
        let mut state=State::empty();
        let a=state.add_group("Source",Bounds{x:100.,y:100.,w:300.,h:300.});
        let b=state.add_group("Target",Bounds{x:500.,y:100.,w:300.,h:300.});
        let source="C:/Desktop/NativePin.lnk".to_string();
        state.group_mut(a).unwrap().tab_mut().items.push(pin(&source,777));
        (state,a,b,source)
    }
    fn temporary_workspace()->PathBuf{
        let marker=std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let path=std::env::temp_dir().join(format!(
            "DesktopCraft-Native-Pin-{}-{marker}",std::process::id()));
        std::fs::create_dir(&path).unwrap();
        path.join("workspace.json")
    }
    #[test]
    fn native_pin_commit_preserves_omitted_default_prefs_and_unknown_json(){
        let path=temporary_workspace();
        let (before,a,b,source)=seed();
        let mut saved=serde_json::to_value(&before).unwrap();
        saved["prefs"].as_object_mut().unwrap().remove("native_explorer_icons");
        saved["future_app_metadata"]=serde_json::json!({
            "version":199,"keep_unknown":true
        });
        let original=serde_json::to_vec_pretty(&saved).unwrap();
        std::fs::write(&path,&original).unwrap();
        let mut after=before.clone();
        after.transfer(a,777,b,0).unwrap();
        commit_verified_native_pin_drop(&path,&before,&after,&source).unwrap();
        let written:serde_json::Value=
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert!(written["prefs"].get("native_explorer_icons").is_none());
        assert_eq!(written["future_app_metadata"],saved["future_app_metadata"]);
        assert_eq!(written["groups"][0]["tabs"][0]["items"].as_array().unwrap().len(),0);
        assert_eq!(written["groups"][1]["tabs"][0]["items"][0]["id"],777);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
    #[test]
    fn pin_commit_after_unsaved_tab_and_box_changes_writes_only_the_pin(){
        let path=temporary_workspace();
        let (mut persisted,a,b,source)=seed();
        persisted.add_tab(b).unwrap();
        persisted.group_mut(b).unwrap().active=0;
        std::fs::write(&path,serde_json::to_vec_pretty(&persisted).unwrap()).unwrap();
        // In-memory only (Native Desktop never saves these): moved Box, other Tab.
        let mut before=persisted.clone();
        before.group_mut(b).unwrap().bounds.x=900.;
        before.group_mut(b).unwrap().active=1;
        let mut after=before.clone();
        after.transfer(a,777,b,0).unwrap();
        assert!(commit_verified_native_pin_drop(&path,&before,&after,&source).is_err());
        commit_verified_native_pin_drop_over(&path,&persisted,&before,&after,&source).unwrap();
        let written=State::load(&path).unwrap();
        let target=written.group(b).unwrap();
        assert_eq!((target.bounds.x,target.active),(500.,0),"unsaved Box move and Tab must not be written");
        assert!(written.group(a).unwrap().tab().items.is_empty());
        assert_eq!(target.tabs[1].items.iter().map(|p|p.id).collect::<Vec<_>>(),vec![777]);
        // Another writer changed the file: still refused.
        let mut other=written.clone();
        other.group_mut(a).unwrap().bounds.y=7.;
        std::fs::write(&path,serde_json::to_vec_pretty(&other).unwrap()).unwrap();
        let _=std::fs::remove_file(backup_path(&path));
        assert!(commit_verified_native_pin_drop_over(&path,&written,&after,&after,&source).is_err());
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
    #[test]
    fn native_pin_transfer_preserves_missing_shortcut_and_its_saved_metadata(){
        let path=temporary_workspace();
        let (mut before,a,b,source)=seed();
        let removed=path.with_file_name("Old Notes.lnk");
        assert!(!removed.try_exists().unwrap());
        before.group_mut(a).unwrap().tab_mut().items.push(
            pin(removed.to_str().unwrap(),4242));
        let mut raw=serde_json::to_value(&before).unwrap();
        raw["groups"][0]["tabs"][0]["items"][1]["future_pin_metadata"]=
            serde_json::json!({"keep":true});
        let original_pin=raw["groups"][0]["tabs"][0]["items"][1].clone();
        std::fs::write(&path,serde_json::to_vec_pretty(&raw).unwrap()).unwrap();
        let mut next=before.clone();
        next.transfer(a,777,b,0).unwrap();
        commit_verified_native_pin_drop(&path,&before,&next,&source).unwrap();
        let saved:serde_json::Value=serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["groups"][0]["tabs"][0]["items"][0],original_pin);
        assert_eq!(saved["groups"][1]["tabs"][0]["items"][0]["id"],777);
        assert_eq!(saved["next_id"],before.next_id);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
    #[test]
    fn rejects_a_pin_added_to_disk_after_native_workspace_acceptance(){
        let path=temporary_workspace();
        let (before,a,b,source)=seed();
        let mut persisted=before.clone();
        let missing=path.with_file_name("Old Notes.lnk");
        persisted.group_mut(a).unwrap().tab_mut().items.push(
            pin(missing.to_str().unwrap(),4242));
        let bytes=serde_json::to_vec_pretty(&persisted).unwrap();
        std::fs::write(&path,&bytes).unwrap();
        let mut next=before.clone();
        next.transfer(a,777,b,0).unwrap();
        assert!(commit_verified_native_pin_drop(&path,&before,&next,&source).is_err());
        assert_eq!(std::fs::read(&path).unwrap(),bytes);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
    #[test]
    fn atomic_native_box_transfer_preserves_exact_predecessor_and_pin_id(){
        let path=temporary_workspace();
        let (before,a,b,source)=seed();
        std::fs::write(&path,serde_json::to_vec_pretty(&before).unwrap()).unwrap();
        let mut next=before.clone();
        next.transfer(a,777,b,0).unwrap();
        let bytes=commit_verified_native_pin_drop(&path,&before,&next,&source).unwrap();
        assert!(bytes>30);
        let saved=State::load(&path).unwrap();
        assert_eq!(serde_json::to_value(&saved).unwrap(),
            serde_json::to_value(&next).unwrap());
        assert_eq!(saved.group(b).unwrap().tab().items[0].id,777);
        assert!(saved.group(a).unwrap().tab().items.is_empty());
        assert!(!path.with_extension("ole-backup").exists());
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
    #[test]
    fn multi_pin_commit_preserves_unknown_json_and_omitted_prefs(){
        let path=temporary_workspace();
        let (mut before,a,b,source1)=seed();
        let source2="C:/Desktop/NativePin2.lnk".to_string();
        before.group_mut(a).unwrap().tab_mut().items.push(pin(&source2,778));
        let mut saved=serde_json::to_value(&before).unwrap();
        saved["prefs"].as_object_mut().unwrap().remove("native_explorer_icons");
        saved["future_app_metadata"]=serde_json::json!({"version":200,"multi":true});
        std::fs::write(&path,serde_json::to_vec_pretty(&saved).unwrap()).unwrap();

        let mut after=before.clone();
        after.transfer(a,777,b,0).unwrap();
        after.transfer(a,778,b,1).unwrap();
        let paths=vec![source1,source2];
        commit_verified_native_pin_drops_over(&path,&before,&before,&after,&paths).unwrap();
        let written:serde_json::Value=
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert!(written["prefs"].get("native_explorer_icons").is_none());
        assert_eq!(written["future_app_metadata"],saved["future_app_metadata"]);
        assert_eq!(written["groups"][0]["tabs"][0]["items"].as_array().unwrap().len(),0);
        let moved_ids:Vec<u64>=written["groups"][1]["tabs"][0]["items"].as_array().unwrap()
            .iter().map(|v|v["id"].as_u64().unwrap()).collect();
        assert_eq!(moved_ids,vec![777,778]);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
    #[test]
    fn same_tab_reorder_is_committed_with_original_pin_metadata() {
        let path = temporary_workspace();
        let (mut before, source, _, first) = seed();
        before.group_mut(source).unwrap().tab_mut().items.push(pin("C:/Desktop/Keep.lnk", 913));
        let mut saved = serde_json::to_value(&before).unwrap();
        saved["groups"][0]["tabs"][0]["items"][0]["future_key"] = true.into();
        std::fs::write(&path, serde_json::to_vec(&saved).unwrap()).unwrap();
        let mut after = before.clone();
        after.group_mut(source).unwrap().tab_mut().items.swap(0, 1);
        commit_verified_native_pin_drops_over(&path, &before, &before, &after, &[first]).unwrap();
        let actual: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(actual["groups"][0]["tabs"][0]["items"][1], saved["groups"][0]["tabs"][0]["items"][0]);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
    #[test]
    fn full_tab_unpin_is_one_workspace_commit() {
        let path = temporary_workspace();
        let (mut before, source, other, first) = seed();
        let mut paths = vec![first];
        for i in 0..16 {
            let item_path = format!("C:/Desktop/GroupPin{i}.lnk");
            before.group_mut(source).unwrap().tab_mut().items.push(pin(&item_path, 1000 + i));
            paths.push(item_path);
        }
        std::fs::write(&path, serde_json::to_vec(&before).unwrap()).unwrap();
        let mut after = before.clone();
        after.group_mut(source).unwrap().tab_mut().items.clear();
        commit_verified_native_pin_drops_over(&path, &before, &before, &after, &paths).unwrap();
        let saved = State::load(&path).unwrap();
        assert!(saved.group(source).unwrap().tab().items.is_empty());
        assert_eq!(saved.group(other), before.group(other));
        assert!(!path.with_extension("ole-backup").exists());
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
    #[test]
    fn multi_pin_commit_after_unsaved_box_move_patches_only_selected_pins(){
        let path=temporary_workspace();
        let (mut persisted,a,b,source1)=seed();
        let source2="C:/Desktop/NativePin2.lnk".to_string();
        persisted.group_mut(a).unwrap().tab_mut().items.push(pin(&source2,778));
        std::fs::write(&path,serde_json::to_vec_pretty(&persisted).unwrap()).unwrap();

        let mut before=persisted.clone();
        before.group_mut(b).unwrap().bounds.x=920.;
        let mut after=before.clone();
        after.transfer(a,777,b,0).unwrap();
        after.transfer(a,778,b,1).unwrap();
        let paths=vec![source1,source2];
        commit_verified_native_pin_drops_over(&path,&persisted,&before,&after,&paths).unwrap();
        let written=State::load(&path).unwrap();
        assert_eq!(written.group(b).unwrap().bounds.x,500.);
        assert_eq!(written.group(b).unwrap().tab().items.iter().map(|p|p.id).collect::<Vec<_>>(),vec![777,778]);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
    #[test]
    fn unrelated_workspace_change_is_rejected_without_writing(){
        let path=temporary_workspace();
        let (before,a,b,source)=seed();
        let original=serde_json::to_vec_pretty(&before).unwrap();
        std::fs::write(&path,&original).unwrap();
        let mut next=before.clone();
        next.transfer(a,777,b,0).unwrap();
        next.tint=!before.tint;
        assert!(commit_verified_native_pin_drop(&path,&before,&next,&source).is_err());
        assert_eq!(std::fs::read(&path).unwrap(),original);
        let mut valid=before.clone();
        valid.transfer(a,777,b,0).unwrap();
        std::fs::write(&path,b"external writer").unwrap();
        assert!(commit_verified_native_pin_drop(&path,&before,&valid,&source).is_err());
        assert_eq!(std::fs::read(&path).unwrap(),b"external writer");
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
    #[test]
    fn native_unpin_commits_only_membership_and_keeps_desktop_file_untouched(){
        let path=temporary_workspace();
        let (before,a,_,source)=seed();
        let original=serde_json::to_vec_pretty(&before).unwrap();
        std::fs::write(&path,&original).unwrap();
        let mut next=before.clone();
        next.unassign(&source);
        commit_verified_native_pin_drop(&path,&before,&next,&source).unwrap();
        assert!(State::load(&path).unwrap().group(a).unwrap().tab().items.is_empty());
        assert!(next.group(a).unwrap().tab().items.is_empty());
        assert!(!path.with_extension("ole-backup").exists());
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
    #[test]
    fn pin_loose_desktop_file_commits_without_copy_or_delete(){
        let path=temporary_workspace();
        let source_path=path.parent().unwrap().join("real-desktop-test-file.txt");
        std::fs::write(&source_path,b"NEVER MOVE REAL SOURCE").unwrap();
        let (before,_,b,_)=seed();
        let original=serde_json::to_vec_pretty(&before).unwrap();
        std::fs::write(&path,&original).unwrap();
        let original_file_bytes=std::fs::read(&source_path).unwrap();
        let mut next=before.clone();
        let id=next.id();
        let pin=Item::from_path(&source_path,id).unwrap();
        next.group_mut(b).unwrap().tab_mut().items.push(pin);
        commit_verified_native_pin_drop(&path,&before,&next,
            &source_path.to_string_lossy()).unwrap();
        assert_eq!(std::fs::read(&source_path).unwrap(),original_file_bytes);
        assert_eq!(State::load(&path).unwrap().group(b).unwrap()
            .tab().items.last().unwrap().id,id);
        assert!(!path.with_extension("ole-backup").exists());
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
    #[test]
    fn unresolved_predecessor_blocks_any_new_native_commit(){
        let path=temporary_workspace();
        let (before,a,b,source)=seed();
        let original=serde_json::to_vec_pretty(&before).unwrap();
        std::fs::write(&path,&original).unwrap();
        let mut next=before.clone();
        next.transfer(a,777,b,0).unwrap();
        std::fs::write(path.with_extension("ole-backup"),b"preserved").unwrap();
        assert!(commit_verified_native_pin_drop(&path,&before,&next,&source)
            .unwrap_err().contains("recovery image"));
        assert_eq!(std::fs::read(&path).unwrap(),original);
        assert_eq!(std::fs::read(path.with_extension("ole-backup")).unwrap(),
            b"preserved");
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Bounds, Item};
    // All cases use an OS temporary directory; never the user's workspace.
    fn item(path: &str) -> Item {
        Item {
            id: 1,
            name: path.into(),
            path: path.into(),
            kind: "lnk".into(),
            size: 0,
            modified: 0,
            label: None,
        }
    }
    fn fixture() -> (State, Vec<ViewItem>, NativeOleViewport, u64, u64) {
        let mut state = State::empty();
        let source = state.add_group(
            "A",
            Bounds {
                x: 60.,
                y: 60.,
                w: 280.,
                h: 240.,
            },
        );
        state
            .group_mut(source)
            .unwrap()
            .tab_mut()
            .items
            .push(item("C:/Lab/A.lnk"));
        state.add_tab(source).unwrap();
        state
            .group_mut(source)
            .unwrap()
            .tab_mut()
            .items
            .push(item("C:/Lab/B.lnk"));
        state.group_mut(source).unwrap().active = 0;
        let target = state.add_group(
            "B",
            Bounds {
                x: 390.,
                y: 60.,
                w: 280.,
                h: 240.,
            },
        );
        state
            .group_mut(target)
            .unwrap()
            .tab_mut()
            .items
            .push(item("C:/Lab/C.lnk"));
        state.add_tab(target).unwrap();
        state
            .group_mut(target)
            .unwrap()
            .tab_mut()
            .items
            .push(item("C:/Lab/D.lnk"));
        state.group_mut(target).unwrap().active = 0;
        let icons = ["A", "B", "C", "D"]
            .iter()
            .enumerate()
            .map(|(i, n)| ViewItem {
                path: format!("C:/Lab/{n}.lnk"),
                label: n.to_string(),
                pos: (i as i32 * 110, 30),
            })
            .collect();
        let v = NativeOleViewport {
            screen_origin_px: (-1100, 300),
            client_size_px: (1000, 500),
            logical_size: (800., 400.),
        };
        (state, icons, v, source, target)
    }
    fn screen() -> (i32, i32) {
        (-1100 + 510, 300 + 190)
    }
    fn offered() -> Vec<PathBuf> {
        vec![PathBuf::from("C:/Lab/A.lnk")]
    }
    struct TempCase {
        root: PathBuf,
        path: PathBuf,
    }
    impl TempCase {
        fn new(state: &State) -> Self {
            use std::sync::atomic::{AtomicU64, Ordering};
            static TEMP_CASE_SEQUENCE: AtomicU64 = AtomicU64::new(1);
            let sequence = TEMP_CASE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "DesktopCraft-OLE-Persist-{}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                sequence
            ));
            fs::create_dir(&root).unwrap();
            let path = root.join("workspace.json");
            state.save(&path).unwrap();
            Self { root, path }
        }
    }

    #[test]
    fn native_destination_pending_crash_backup_is_detected_and_blocks_any_save_or_backup(){
        let case=TempCase::new(&State::empty());
        let initial=read_existing(&case.path).unwrap();
        let before=case.path.with_extension("native-target-backup");
        fs::write(&before,&initial).unwrap();
        let inspection=inspect_native_ole_recovery(&case.path);
        assert!(inspection.native_target_backup_exists);
        assert!(inspection.native_target_backup_valid);
        assert!(inspection.needs_manual_review);
        assert!(State::empty().save(&case.path).is_err());
        assert!(create_verified_workspace_safety_checkpoint(
            &case.path,&case.root.join("not-allowed.json")).is_err());
        assert!(!case.root.join("not-allowed.json").exists());
        assert_eq!(read_existing(&case.path).unwrap(),initial);
    }
    #[test]
    fn rolled_back_postimage_is_reported_as_pending_manual_recovery(){
        let case=TempCase::new(&State::empty());
        let original=read_existing(&case.path).unwrap();
        let last=case.path.with_extension("rollback-after");
        fs::write(&last,&original).unwrap();
        let status=inspect_native_ole_recovery(&case.path);
        assert!(status.rollback_after_exists);
        assert!(status.rollback_after_valid);
        assert!(status.needs_manual_review);
        assert!(!status.native_target_backup_exists);
        assert!(State::empty().save(&case.path).is_err());
        assert_eq!(read_existing(&case.path).unwrap(),original);
    }

    #[test]
    fn checkpoint_backed_native_destination_appends_one_box_and_preserves_predecessor() {
        let original=State::empty();
        let case=TempCase::new(&original);
        let checkpoint=case.root.join("user-checkpoint.json");
        create_verified_workspace_safety_checkpoint(&case.path,&checkpoint).unwrap();
        let original_bytes=read_existing(&case.path).unwrap();
        let mut proposed=original.clone();
        let target=proposed.add_group("Native Desktop",
            crate::model::Bounds{x:800.,y:400.,w:360.,h:270.});
        proposed.add_tab(target).unwrap();
        proposed.group_mut(target).unwrap().active=0;
        let after=case.root.join("expected-post.json");
        let len=commit_checked_native_destination(
            &case.path,&checkpoint,&after,&original,&proposed).unwrap();
        assert!(len>original_bytes.len());
        assert_eq!(read_existing(&checkpoint).unwrap(),original_bytes);
        assert_eq!(read_existing(&after).unwrap(),read_existing(&case.path).unwrap());
        assert_eq!(State::load(&case.path).unwrap().groups.len(),1);
        assert!(!case.path.with_extension("native-target-backup").exists());
        assert!(unresolved_workspace_ole_stages(&case.path).unwrap().is_empty());
        assert!(commit_checked_native_destination(
            &case.path,&checkpoint,&case.root.join("second-post.json"),
            &original,&proposed).is_err(),"Cannot replay an old checkpoint after commit");
    }
    #[test]
    fn checkpoint_backed_target_refuses_changed_live_workspace_and_modified_original_box(){
        let original=State::empty();
        let case=TempCase::new(&original);
        let checkpoint=case.root.join("checkpoint.json");
        create_verified_workspace_safety_checkpoint(&case.path,&checkpoint).unwrap();
        let mut proposed=original.clone();
        let target=proposed.add_group("Native Desktop",
            crate::model::Bounds{x:800.,y:400.,w:360.,h:270.});
        proposed.add_tab(target).unwrap();
        proposed.group_mut(target).unwrap().active=0;
        let mut external=original.clone();
        external.add_group("User edit",
            crate::model::Bounds{x:200.,y:200.,w:320.,h:230.});
        external.save(&case.path).unwrap();
        let after=case.root.join("should-not-exist.json");
        let fresh=read_existing(&case.path).unwrap();
        assert!(commit_checked_native_destination(
            &case.path,&checkpoint,&after,&original,&proposed).is_err());
        assert_eq!(read_existing(&case.path).unwrap(),fresh);
        assert!(!after.exists());
        // Restore only inside the disposable test fixture; then alter a
        // non-new State field. Even an otherwise valid new Box must fail.
        original.save(&case.path).unwrap();
        proposed.snap=!original.snap;
        assert!(commit_checked_native_destination(
            &case.path,&checkpoint,&after,&original,&proposed).is_err());
        assert!(!after.exists());
        let mut invalid=proposed.clone();
        invalid.snap=original.snap;
        invalid.groups[0].tabs[0].items.push(item("C:/desktop/new.lnk"));
        assert!(commit_checked_native_destination(
            &case.path,&checkpoint,&after,&original,&invalid).is_err());
        assert_eq!(read_existing(&case.path).unwrap(),read_existing(&checkpoint).unwrap());
    }
    #[test]
    fn checkpoint_backed_target_refuses_orphan_stage_and_normal_save_refuses_predecessor(){
        let case=TempCase::new(&State::empty());
        let checkpoint=case.root.join("checkpoint.json");
        create_verified_workspace_safety_checkpoint(&case.path,&checkpoint).unwrap();
        let mut proposed=State::empty();
        let target=proposed.add_group("Native Desktop",
            crate::model::Bounds{x:800.,y:400.,w:360.,h:270.});
        proposed.add_tab(target).unwrap();
        proposed.group_mut(target).unwrap().active=0;
        let pending=case.path.with_extension("native-target-backup");
        fs::write(&pending,b"original failed replacement").unwrap();
        assert!(commit_checked_native_destination(&case.path,&checkpoint,
            &case.root.join("post.json"),&State::empty(),&proposed).is_err());
        assert!(State::empty().save(&case.path).is_err(),
            "Normal app saves must refuse unresolved Native destination predecessor");
        fs::remove_file(&pending).unwrap();
        let stage=StageFile::new(&case.path).unwrap();
        assert!(commit_checked_native_destination(&case.path,&checkpoint,
            &case.root.join("post.json"),&State::empty(),&proposed).is_err());
        drop(stage);
        let preserved=case.path.with_extension("rollback-after");
        fs::write(&preserved,b"review").unwrap();
        assert!(State::empty().save(&case.path).is_err());
        assert!(!case.root.join("post.json").exists());
    }

    #[test]
    fn user_safety_checkpoint_creates_new_exact_copy_without_touching_workspace() {
        let case=TempCase::new(&State::empty());
        let baseline=read_existing(&case.path).unwrap();
        let output=case.root.join("verified-original-checkpoint.json");
        assert_eq!(create_verified_workspace_safety_checkpoint(&case.path,&output).unwrap(),
            baseline.len());
        assert_eq!(read_existing(&output).unwrap(),baseline);
        assert_eq!(read_existing(&case.path).unwrap(),baseline);
        assert!(create_verified_workspace_safety_checkpoint(&case.path,&output).is_err(),
            "A second checkpoint must NOT overwrite an existing file");
        assert_eq!(read_existing(&output).unwrap(),baseline);
        assert!(!backup_path(&case.path).exists());
        assert!(!case.path.with_extension("rollback-after").exists());
    }

    #[test]
    fn user_safety_rollback_requires_exact_expected_current_and_preserves_overwritten_image(){
        let case=TempCase::new(&State::empty());
        let original=read_existing(&case.path).unwrap();
        let backup=case.root.join("original-checkpoint.json");
        create_verified_workspace_safety_checkpoint(&case.path,&backup).unwrap();
        let mut post=State::empty();
        post.add_group("Committed Native Destination",
            crate::model::Bounds{x:500.,y:100.,w:300.,h:230.});
        post.save(&case.path).unwrap();
        let after=read_existing(&case.path).unwrap();
        assert_ne!(after,original);
        let expected=case.root.join("expected-current.json");
        fs::write(&expected,&after).unwrap();
        let bytes=rollback_workspace_safety_checkpoint(&case.path,&backup,&expected).unwrap();
        assert_eq!(bytes,original.len());
        assert_eq!(read_existing(&case.path).unwrap(),original);
        assert_eq!(read_existing(&case.path.with_extension("rollback-after")).unwrap(),after);
        assert_eq!(read_existing(&backup).unwrap(),original);
        assert_eq!(read_existing(&expected).unwrap(),after);
        assert!(rollback_workspace_safety_checkpoint(&case.path,&backup,&expected).is_err(),
            "Replay must reject because current Workspace no longer matches expected post state");
    }

    #[test]
    fn safety_rollback_rejects_intervening_user_layout_change_without_ever_overwriting_it(){
        let case=TempCase::new(&State::empty());
        let backup=case.root.join("original-checkpoint.json");
        create_verified_workspace_safety_checkpoint(&case.path,&backup).unwrap();
        let mut post=State::empty();
        post.add_group("Expected post",crate::model::Bounds{x:500.,y:50.,w:310.,h:220.});
        post.save(&case.path).unwrap();
        let expected=case.root.join("expected-post.json");
        fs::write(&expected,read_existing(&case.path).unwrap()).unwrap();
        let mut changed=post.clone();
        changed.add_group("User's later edit",
            crate::model::Bounds{x:120.,y:400.,w:310.,h:230.});
        changed.save(&case.path).unwrap();
        let latest=read_existing(&case.path).unwrap();
        assert!(rollback_workspace_safety_checkpoint(&case.path,&backup,&expected).is_err());
        assert_eq!(read_existing(&case.path).unwrap(),latest);
        assert!(!case.path.with_extension("rollback-after").exists());
        assert!(unresolved_workspace_ole_stages(&case.path).unwrap().is_empty());
    }

    #[test]
    fn safety_checkpoint_and_rollback_refuse_corrupted_or_pending_recovery_artifacts(){
        let case=TempCase::new(&State::empty());
        let output=case.root.join("verified.json");
        let mut lease=WorkspaceLease::acquire(&case.path).unwrap();
        assert!(create_verified_workspace_safety_checkpoint(&case.path,&output).is_err());
        drop(lease);
        let orig=read_existing(&case.path).unwrap();
        let stage=StageFile::new(&case.path).unwrap();
        assert!(create_verified_workspace_safety_checkpoint(&case.path,&output).is_err());
        drop(stage);
        create_verified_workspace_safety_checkpoint(&case.path,&output).unwrap();
        let corrupted=case.root.join("corrupted.json");
        fs::write(&corrupted,b"{").unwrap();
        let expected=case.root.join("expected.json");
        fs::write(&expected,&orig).unwrap();
        assert!(rollback_workspace_safety_checkpoint(&case.path,&corrupted,&expected).is_err());
        let recovery=case.path.with_extension("rollback-after");
        fs::write(&recovery,&orig).unwrap();
        assert!(create_verified_workspace_safety_checkpoint(
            &case.path,&case.root.join("no-new-checkpoint.json")).is_err());
        assert!(rollback_workspace_safety_checkpoint(&case.path,&output,&expected).is_err());
        assert_eq!(read_existing(&case.path).unwrap(),orig);
    }
    impl Drop for TempCase {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
    fn begin(
        t: &TempCase,
        state: &State,
        icons: &[ViewItem],
        v: NativeOleViewport,
        source: u64,
        target: u64,
    ) -> NativeOlePersistedGesture {
        NativeOlePersistedGesture::begin(
            &t.path,
            state,
            icons,
            "DISP@125%",
            v,
            &offered(),
            &[source, target],
            4,
            3,
        )
        .unwrap()
    }

    #[test]
    fn persists_exact_virtual_transfer_to_disposable_workspace() {
        let (mut state, icons, v, source, target) = fixture();
        let t = TempCase::new(&state);
        let gesture = begin(&t, &state, &icons, v, source, target);
        let plan = gesture
            .commit(
                &mut state,
                &icons,
                "DISP@125%",
                v,
                &[source, target],
                &offered(),
                screen(),
                7,
            )
            .unwrap();
        let saved = State::load(&t.path).unwrap();
        assert!(saved.group(source).unwrap().tabs[0].items.is_empty());
        assert_eq!(saved.group(target).unwrap().tabs[0].items.len(), 2);
        assert_eq!(
            serde_json::to_vec(&saved).unwrap(),
            serde_json::to_vec(&state).unwrap()
        );
        assert_eq!(plan.hidden_for(&[]).unwrap(), vec![1, 3]);
        let artifacts = fs::read_dir(&t.root).unwrap().count();
        assert_eq!(artifacts, 1); // no backup/stage/lock leak
    }

    #[test]
    fn two_selected_native_pins_commit_as_one_durable_workspace_transaction() {
        let (mut state,mut icons,v,source,target)=fixture();
        // Both selected paths belong to the same active Source Tab.
        // Fixtures deliberately reuse numeric item IDs, as legacy files can.
        state.group_mut(source).unwrap().tabs[0].items.push(item("C:/Lab/E.lnk"));
        icons.push(ViewItem{path:"C:/Lab/E.lnk".into(),
            label:"E".into(),pos:(530,30)});
        let selected=vec![PathBuf::from("C:/Lab/A.lnk"),
                          PathBuf::from("C:/Lab/E.lnk")];
        let t=TempCase::new(&state);
        let original=read_existing(&t.path).unwrap();
        let ticket=NativeOlePersistedGesture::begin(
            &t.path,&state,&icons,"DISP@125%",v,&selected,
            &[source,target],4,3).unwrap();
        let plan=ticket.commit(&mut state,&icons,"DISP@125%",v,
            &[source,target],&selected,screen(),7).unwrap();
        let committed=read_existing(&t.path).unwrap();
        assert_ne!(original,committed);
        let disk=State::load(&t.path).unwrap();
        assert_eq!(serialized_state(&disk).unwrap(),
                   serialized_state(&state).unwrap());
        assert_eq!(disk.group(source).unwrap().tabs[0].items.len(),0);
        let target_pins=&disk.group(target).unwrap().tabs[0].items;
        assert_eq!(target_pins.len(),3);
        assert_eq!(target_pins[1].path,"C:/Lab/A.lnk");
        assert_eq!(target_pins[2].path,"C:/Lab/E.lnk");
        assert_eq!(plan.groups.len(),2);
        assert_eq!(fs::read_dir(&t.root).unwrap().count(),1,
            "Neither OLE backup, staged temp file, nor lease may remain");

        // No source file path or Explorer native coordinate has been changed;
        // only the temporary virtual-pin Workspace was saved.
        assert_eq!(icons.last().unwrap().pos,(530,30));
        assert_eq!(selected.len(),2);
    }

    #[test]
    fn two_selected_pins_never_partially_save_if_capacity_or_source_changes() {
        let (mut state,mut icons,v,source,target)=fixture();
        state.group_mut(source).unwrap().tabs[0].items.push(item("C:/Lab/E.lnk"));
        icons.push(ViewItem{path:"C:/Lab/E.lnk".into(),
            label:"E".into(),pos:(530,30)});
        let selected=vec![PathBuf::from("C:/Lab/A.lnk"),
                          PathBuf::from("C:/Lab/E.lnk")];
        let t=TempCase::new(&state);
        let unchanged=read_existing(&t.path).unwrap();
        let ticket=NativeOlePersistedGesture::begin(&t.path,&state,&icons,
            "DISP@125%",v,&selected,&[source,target],4,2).unwrap();
        assert!(ticket.commit(&mut state,&icons,
            "DISP@125%",v,&[source,target],&selected,screen(),7).is_err());
        assert_eq!(read_existing(&t.path).unwrap(),unchanged);
        assert_eq!(serialized_state(&State::load(&t.path).unwrap()).unwrap(),
                   serialized_state(&state).unwrap());
        assert_eq!(fs::read_dir(&t.root).unwrap().count(),1);
        let next=NativeOlePersistedGesture::begin(&t.path,&state,&icons,
            "DISP@125%",v,&selected,&[source,target],4,3).unwrap();
        let substituted=vec![selected[0].clone(),PathBuf::from("C:/Lab/B.lnk")];
        assert!(next.commit(&mut state,&icons,"DISP@125%",v,
            &[source,target],&substituted,screen(),7).is_err());
        assert_eq!(read_existing(&t.path).unwrap(),unchanged);
        assert_eq!(fs::read_dir(&t.root).unwrap().count(),1);
    }

    #[test]
    fn external_workspace_edit_aborts_without_overwriting_disk_or_live_state() {
        let (mut state, icons, v, source, target) = fixture();
        let t = TempCase::new(&state);
        let gesture = begin(&t, &state, &icons, v, source, target);
        let unchanged = serialized_state(&state).unwrap();
        let mut writer = state.clone();
        writer.tint = !writer.tint;
        fs::write(&t.path, serde_json::to_vec_pretty(&writer).unwrap()).unwrap();
        let external = fs::read(&t.path).unwrap();
        assert!(gesture
            .commit(
                &mut state,
                &icons,
                "DISP@125%",
                v,
                &[source, target],
                &offered(),
                screen(),
                7
            )
            .is_err());
        assert_eq!(fs::read(&t.path).unwrap(), external);
        assert_eq!(serialized_state(&state).unwrap(), unchanged);
        assert_eq!(fs::read_dir(&t.root).unwrap().count(), 1);
    }

    #[test]
    fn cooperative_lock_and_leftover_backup_abort_before_workspace_write() {
        let (mut state, icons, v, source, target) = fixture();
        let t = TempCase::new(&state);
        let before = read_existing(&t.path).unwrap();
        let gesture = begin(&t, &state, &icons, v, source, target);
        let lock = WorkspaceLease::acquire(&t.path).unwrap();
        assert!(gesture
            .commit(
                &mut state,
                &icons,
                "DISP@125%",
                v,
                &[source, target],
                &offered(),
                screen(),
                7
            )
            .is_err());
        assert_eq!(read_existing(&t.path).unwrap(), before);
        drop(lock);
        assert!(!lock_path(&t.path).exists());
        let recovery = backup_path(&t.path);
        fs::write(&recovery, b"Prior recovery copy").unwrap();
        let gesture = begin(&t, &state, &icons, v, source, target);
        assert!(gesture
            .commit(
                &mut state,
                &icons,
                "DISP@125%",
                v,
                &[source, target],
                &offered(),
                screen(),
                7
            )
            .is_err());
        assert_eq!(read_existing(&t.path).unwrap(), before);
        assert_eq!(fs::read(&recovery).unwrap(), b"Prior recovery copy");
        assert!(!lock_path(&t.path).exists());
    }

    #[test]
    fn standard_state_save_respects_ole_workspace_lease() {
        let (state, _, _, _, _) = fixture();
        let t = TempCase::new(&state);
        let before = read_existing(&t.path).unwrap();
        let lease = WorkspaceLease::acquire(&t.path).unwrap();
        let mut edited = state.clone();
        edited.tint = !edited.tint;
        assert!(edited.save(&t.path).is_err());
        assert_eq!(read_existing(&t.path).unwrap(), before);
        drop(lease);
        assert!(!lock_path(&t.path).exists());
        edited.save(&t.path).unwrap();
        assert_ne!(read_existing(&t.path).unwrap(), before);
        assert!(!lock_path(&t.path).exists());
    }

    #[test]
    fn crash_artifact_inspection_is_read_only_and_never_overwrites_layout() {
        let (state, _, _, _, _) = fixture();
        let t = TempCase::new(&state);
        let pristine = fs::read(&t.path).unwrap();
        let initial = inspect_native_ole_recovery(&t.path);
        assert!(initial.workspace_valid && !initial.needs_manual_review);
        assert!(!initial.backup_exists && !initial.lease_present);
        let backup = backup_path(&t.path);
        fs::write(&backup, &pristine).unwrap();
        let lock = WorkspaceLease::acquire(&t.path).unwrap();
        let report = inspect_native_ole_recovery(&t.path);
        assert!(report.workspace_valid && report.backup_valid);
        assert!(report.lease_present && report.needs_manual_review);
        assert_eq!(fs::read(&t.path).unwrap(), pristine);
        assert_eq!(fs::read(&backup).unwrap(), pristine);
        drop(lock);
        fs::write(&t.path, b"not valid json").unwrap();
        let broken = inspect_native_ole_recovery(&t.path);
        assert!(!broken.workspace_valid);
        assert!(broken.backup_valid);
        assert!(broken.needs_manual_review);
        assert_eq!(fs::read(&t.path).unwrap(), b"not valid json");
        assert_eq!(fs::read(&backup).unwrap(), pristine); // no self-repair
    }

    #[test]
    fn orphaned_ole_stage_is_detected_and_blocks_both_save_and_drop() {
        let (mut state, icons, v, source, target) = fixture();
        let t=TempCase::new(&state);
        let original=read_existing(&t.path).unwrap();
        let gesture=begin(&t,&state,&icons,v,source,target);
        let mut newer=state.clone();
        newer.tint=!newer.tint;
        let next_bytes=serde_json::to_vec_pretty(&newer).unwrap();
        let mut stage=StageFile::new(&t.path).unwrap();
        stage.stage(&next_bytes).unwrap();
        let orphan=stage.path.clone();
        // Test a previous process that lost track of its flushed temporary
        // transaction without committing. Nothing touches the user desktop.
        std::mem::forget(stage);
        let inspected=inspect_native_ole_recovery(&t.path);
        assert!(inspected.workspace_valid);
        assert!(inspected.needs_manual_review);
        assert!(inspected.stage_scan_ok);
        assert_eq!(inspected.staged_artifact_count,1);
        assert!(inspected.single_stage_valid);
        let saved_err=newer.save(&t.path).unwrap_err();
        assert!(saved_err.contains("unresolved OLE staged"));
        assert!(gesture.commit(
            &mut state,&icons,"DISP@125%",v,&[source,target],
            &offered(),screen(),7
        ).unwrap_err().contains("Unresolved OLE staged"));
        assert_eq!(read_existing(&t.path).unwrap(),original);
        assert_eq!(read_existing(&orphan).unwrap(),next_bytes);
        let exported=t.root.join("explicit-ole-stage-export.json");
        export_native_ole_stage(&t.path,&exported).unwrap();
        assert_eq!(read_existing(&exported).unwrap(),next_bytes);
        assert!(export_native_ole_stage(&t.path,&exported).is_err());
        assert!(export_native_ole_stage(&t.path,&t.path).is_err());
        assert!(export_native_ole_stage(&t.path,&orphan).is_err());
        assert_eq!(read_existing(&orphan).unwrap(),next_bytes);
        assert!(!lock_path(&t.path).exists());

        // Another file's staged OLE transaction cannot poison this one.
        let unrelated=t.root.join(".another-workspace.json.desktopcraft-ole-xyz.tmp");
        fs::write(&unrelated,b"unrelated").unwrap();
        assert_eq!(unresolved_workspace_ole_stages(&t.path).unwrap().len(),1);

        // Only the TEST may remove its synthetic orphan to prove normal
        // saving resumes after an explicit user-reviewed recovery decision.
        fs::remove_file(&orphan).unwrap();
        assert_eq!(unresolved_workspace_ole_stages(&t.path).unwrap().len(),0);
        assert!(!inspect_native_ole_recovery(&t.path).needs_manual_review);
        newer.save(&t.path).unwrap();
        assert_ne!(read_existing(&t.path).unwrap(),original);
    }

    #[test]
    fn invalid_and_multiple_stages_are_reported_but_never_destroyed() {
        let (state,_,_,_,_)=fixture();
        let t=TempCase::new(&state);
        let original=read_existing(&t.path).unwrap();
        let prefix=workspace_stage_prefix(&t.path).unwrap();
        let a=t.root.join(format!("{prefix}100.tmp"));
        let b=t.root.join(format!("{prefix}200.tmp"));
        fs::write(&a,b"damaged interrupted OLE transaction").unwrap();
        let report=inspect_native_ole_recovery(&t.path);
        assert_eq!(report.staged_artifact_count,1);
        assert!(!report.single_stage_valid);
        assert!(report.needs_manual_review);
        fs::write(&b,&original).unwrap();
        let report=inspect_native_ole_recovery(&t.path);
        assert_eq!(report.staged_artifact_count,2);
        assert!(export_native_ole_stage(&t.path,&t.root.join("ambiguous.json")).is_err());
        assert!(!t.root.join("ambiguous.json").exists());
        assert!(!report.single_stage_valid);
        assert!(report.stage_scan_ok);
        assert_eq!(read_existing(&t.path).unwrap(),original);
        assert_eq!(read_existing(&a).unwrap(),b"damaged interrupted OLE transaction");
        assert_eq!(read_existing(&b).unwrap(),original);
        assert!(state.save(&t.path).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn lease_crash_worker() {
        let Ok(path) = std::env::var("DC_OLE_LEASE_CRASH_FIXTURE") else {
            return; // normal test run must never hold a real workspace lease
        };
        let path = PathBuf::from(path);
        assert!(path.to_string_lossy().contains("DesktopCraft-OLE-Persist-"));
        let _lease = WorkspaceLease::acquire(&path).unwrap();
        // The parent test terminates this disposable child process. There is
        // no live Explorer, no user Workspace and no desktop input.
        std::thread::sleep(std::time::Duration::from_secs(30));
    }

    #[cfg(windows)]
    #[test]
    fn terminated_writer_releases_lease_without_orphaned_file() {
        use std::process::{Command, Stdio};
        use std::time::{Duration, Instant};
        let (state, _, _, _, _) = fixture();
        let t = TempCase::new(&state);
        let before = read_existing(&t.path).unwrap();
        let exe = std::env::current_exe().unwrap();
        let mut child = Command::new(exe)
            .args([
                "--exact",
                "native_ole_persist::tests::lease_crash_worker",
                "--nocapture",
            ])
            .env("DC_OLE_LEASE_CRASH_FIXTURE", &t.path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut held = false;
        while Instant::now() < deadline {
            if lock_path(&t.path).exists() {
                held = true;
                break;
            }
            if child.try_wait().unwrap().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        let _ = child.kill();
        let _ = child.wait();
        assert!(held, "Isolated worker did not take the OLE lease");
        let deadline = Instant::now() + Duration::from_secs(3);
        while lock_path(&t.path).exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(25));
        }
        assert!(
            !lock_path(&t.path).exists(),
            "Killed writer stranded its lease"
        );
        assert_eq!(read_existing(&t.path).unwrap(), before);
        let _reopened = WorkspaceLease::acquire(&t.path).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn interrupted_stage_crash_worker() {
        let Ok(path)=std::env::var("DC_OLE_STAGE_CRASH_FIXTURE") else {
            return; // ordinary test runs never stage an external Workspace
        };
        let workspace=PathBuf::from(path);
        let parent=workspace.parent().unwrap();
        let expected=std::env::temp_dir().canonicalize().unwrap();
        assert!(parent.canonicalize().unwrap().starts_with(expected));
        assert!(parent.file_name().unwrap().to_string_lossy().starts_with(
            "DesktopCraft-OLE-Persist-"));
        let state=State::load(&workspace).unwrap();
        let mut next=state.clone();
        next.tint=!next.tint;
        let mut stage=StageFile::new(&workspace).unwrap();
        stage.stage(&serde_json::to_vec_pretty(&next).unwrap()).unwrap();
        // Parent force-kills this child. The flushed stage must remain
        // recoverable; no user Workspace or Explorer process is involved.
        std::thread::sleep(std::time::Duration::from_secs(30));
    }

    #[cfg(windows)]
    #[test]
    fn killed_staging_process_leaves_recoverable_transaction_without_overwrite() {
        use std::process::{Command,Stdio};
        use std::time::{Duration,Instant};
        let (state,_,_,_,_)=fixture();
        let t=TempCase::new(&state);
        let original=read_existing(&t.path).unwrap();
        let exe=std::env::current_exe().unwrap();
        let mut worker=Command::new(exe)
            .args(["--exact",
                "native_ole_persist::tests::interrupted_stage_crash_worker",
                "--nocapture"])
            .env("DC_OLE_STAGE_CRASH_FIXTURE",&t.path)
            .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
            .spawn().unwrap();
        let deadline=Instant::now()+Duration::from_secs(5);
        let mut pending=Vec::new();
        let mut stage_fully_persisted=false;
        while Instant::now()<deadline {
            pending=unresolved_workspace_ole_stages(&t.path).unwrap();
            if pending.len()==1 && read_existing(&pending[0])
                .and_then(|bytes|State::from_saved_bytes(&bytes).map(|_|())).is_ok()
            {
                // The worker has finished writing and flushing a COMPLETE
                // state; only now should the parent simulate termination.
                stage_fully_persisted=true;
                break;
            }
            if worker.try_wait().unwrap().is_some(){break;}
            std::thread::sleep(Duration::from_millis(25));
        }
        let _=worker.kill();
        let _=worker.wait();
        assert!(stage_fully_persisted,"Child did not finish a valid OLE stage");
        assert_eq!(pending.len(),1,"Child never staged OLE transaction");
        let status=inspect_native_ole_recovery(&t.path);
        assert!(status.workspace_valid && status.single_stage_valid);
        assert!(status.needs_manual_review);
        assert_eq!(read_existing(&t.path).unwrap(),original);
        assert!(state.save(&t.path).is_err());
        let exported=t.root.join("crashed-child-staged.json");
        export_native_ole_stage(&t.path,&exported).unwrap();
        assert_ne!(read_existing(&exported).unwrap(),original);
        assert_eq!(read_existing(&pending[0]).unwrap(),read_existing(&exported).unwrap());
        fs::remove_file(&pending[0]).unwrap(); // test-only reviewed cleanup
        state.save(&t.path).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn stale_legacy_sidecar_does_not_permanently_block_normal_saves() {
        let (state, _, _, _, _) = fixture();
        let t = TempCase::new(&state);
        let sidecar = lock_path(&t.path);
        fs::write(&sidecar, b"Legacy crash marker").unwrap();
        let lease = WorkspaceLease::acquire(&t.path).unwrap();
        let other = std::thread::spawn({
            let path = t.path.clone();
            move || WorkspaceLease::acquire(&path).is_err()
        });
        assert!(
            other.join().unwrap(),
            "Exclusive Windows lease not enforced"
        );
        drop(lease);
        assert!(!sidecar.exists(), "Legacy sidecar must be deleted on close");
        state.save(&t.path).unwrap();
        assert!(!sidecar.exists());
    }

    #[test]
    fn normal_save_refuses_orphaned_recovery_backup() {
        let (mut state, _, _, _, _) = fixture();
        let t = TempCase::new(&state);
        let before = read_existing(&t.path).unwrap();
        let backup = backup_path(&t.path);
        fs::write(&backup, &before).unwrap();
        state.blend = !state.blend;
        let error = state
            .save(&t.path)
            .expect_err("Crash backup should halt normal save");
        assert!(error.contains("recovery backup"), "{error}");
        assert_eq!(read_existing(&t.path).unwrap(), before);
        assert_eq!(read_existing(&backup).unwrap(), before);
        assert!(!lock_path(&t.path).exists());
        fs::remove_file(backup).unwrap();
        state.save(&t.path).unwrap();
        assert_ne!(read_existing(&t.path).unwrap(), before);
    }

    #[test]
    fn recovery_export_creates_new_verified_copy_without_touching_workspace() {
        let (state, _, _, _, _) = fixture();
        let t = TempCase::new(&state);
        let live_bytes = read_existing(&t.path).unwrap();
        let backup = backup_path(&t.path);
        assert!(export_native_ole_backup(&t.path, &t.root.join("new.json")).is_err());
        let mut earlier = state.clone();
        earlier.tint = !earlier.tint;
        earlier.save(&t.root.join("old-state.json")).unwrap();
        let older = read_existing(&t.root.join("old-state.json")).unwrap();
        fs::write(&backup, &older).unwrap();

        let exported = t.root.join("inspect-copy.json");
        export_native_ole_backup(&t.path, &exported).unwrap();
        assert_eq!(read_existing(&exported).unwrap(), older);
        assert_eq!(read_existing(&t.path).unwrap(), live_bytes);
        assert_eq!(read_existing(&backup).unwrap(), older);
        assert!(export_native_ole_backup(&t.path, &exported).is_err());
        assert!(export_native_ole_backup(&t.path, &t.path).is_err());
        assert!(export_native_ole_backup(&t.path, &backup).is_err());
        fs::write(&backup, b"broken backup data").unwrap();
        assert!(export_native_ole_backup(&t.path, &t.root.join("invalid.json")).is_err());
        assert!(!t.root.join("invalid.json").exists());
        assert_eq!(read_existing(&t.path).unwrap(), live_bytes);
    }

    #[test]
    fn detects_uncooperative_writer_between_final_check_and_replacefile() {
        // All paths are in a unique disposable OS TEMP directory. The
        // external writer deliberately ignores DesktopCraft's sidecar lock.
        let (state,_,_,_,_)=fixture();
        let t=TempCase::new(&state);
        let original=read_existing(&t.path).unwrap();
        let mut next=state.clone();
        next.tint=!next.tint;
        let next_bytes=serde_json::to_vec_pretty(&next).unwrap();
        let mut stage=StageFile::new(&t.path).unwrap();
        stage.stage(&next_bytes).unwrap();

        // Exact worst-case race: after the last baseline comparison but
        // immediately before ReplaceFileW replaces the active Workspace.
        let mut external=state.clone();
        external.blend=!external.blend;
        let outside_bytes=serde_json::to_vec_pretty(&external).unwrap();
        fs::write(&t.path,&outside_bytes).unwrap();
        let backup=backup_path(&t.path);
        replace_preserving_backup(&t.path,&stage.path,&backup).unwrap();
        let result=verify_replaced_predecessor(&backup,&original);
        assert!(result.is_err(),"External writer was missed");
        assert!(result.unwrap_err().contains("writer raced"));
        // We never call it a successful OLE Drop. The overwritten external
        // writer version is retained intact for explicit user recovery.
        assert_eq!(read_existing(&backup).unwrap(),outside_bytes);
        assert_eq!(read_existing(&t.path).unwrap(),next_bytes);
        assert!(state.save(&t.path).is_err(),"Regular saves must halt");
        let recovery=t.root.join("external-writer-recovered.json");
        export_native_ole_backup(&t.path,&recovery).unwrap();
        assert_eq!(read_existing(&recovery).unwrap(),outside_bytes);
        assert_eq!(read_existing(&backup).unwrap(),outside_bytes);
    }

    #[test]
    fn rejects_oversize_workspace_before_attempting_to_parse_or_commit() {
        let (state, _, _, _, _) = fixture();
        let t = TempCase::new(&state);
        let mut f = OpenOptions::new().write(true).open(&t.path).unwrap();
        f.set_len(32 * 1024 * 1024 + 10).unwrap();
        drop(f);
        assert!(read_existing(&t.path).is_err());
        assert!(State::load(&t.path).is_err(), "All Workspace loads must be bounded");
        assert!(inspect_native_ole_recovery(&t.path).needs_manual_review);
        // The original data still exists; the tool does not truncate it.
        assert_eq!(fs::metadata(&t.path).unwrap().len(), 32 * 1024 * 1024 + 10);
    }

    #[test]
    fn recovery_export_validates_the_exact_bytes_it_writes() {
        let (state,_,_,_,_)=fixture();
        let t=TempCase::new(&state);
        let backup=backup_path(&t.path);
        let original=read_existing(&t.path).unwrap();
        let parsed=State::from_saved_bytes(&original).unwrap();
        assert_eq!(serialized_state(&parsed).unwrap(),
                   serialized_state(&state).unwrap());
        fs::write(&backup,&original).unwrap();
        let dest=t.root.join("exact-validated-copy.json");
        export_native_ole_backup(&t.path,&dest).unwrap();
        assert_eq!(read_existing(&dest).unwrap(),original);
        // Source now changes: the bytes from the previous read remain a
        // valid independent snapshot, but the new corrupted source must
        // be rejected, never writing a new export.
        fs::write(&backup,b"{broken json").unwrap();
        assert!(State::from_saved_bytes(&original).is_ok());
        let denied=t.root.join("invalid-after-change.json");
        assert!(export_native_ole_backup(&t.path,&denied).is_err());
        assert!(!denied.exists());
        assert_eq!(read_existing(&dest).unwrap(),original);
        assert_eq!(read_existing(&backup).unwrap(),b"{broken json");
    }

    #[test]
    fn persisted_ole_session_only_acknowledges_link_after_successful_disk_commit() {
        let (mut live, icons, viewport, source, target) = fixture();
        let case = TempCase::new(&live);
        let ids = [source, target];
        let mut session = NativeOlePersistedSession::default();
        assert!(session
            .drop(
                &mut live,
                &icons,
                "DISP@125%",
                viewport,
                &ids,
                &offered(),
                screen(),
                7
            )
            .is_err()); // no DragEnter
        session
            .drag_enter(
                &case.path,
                &live,
                &icons,
                "DISP@125%",
                viewport,
                &offered(),
                &ids,
                4,
                3,
            )
            .unwrap();
        assert_eq!(
            session.drag_over_effect(
                &live,
                &icons,
                "DISP@125%",
                viewport,
                &ids,
                &offered(),
                screen(),
                7
            ),
            4
        );
        assert_eq!(
            session.drag_over_effect(
                &live,
                &icons,
                "DISP@125%",
                viewport,
                &ids,
                &offered(),
                screen(),
                2
            ),
            0
        );
        assert_eq!(
            session.drag_over_effect(
                &live,
                &icons,
                "DISP@125%",
                viewport,
                &ids,
                &offered(),
                (-40000, 0),
                7
            ),
            0
        );
        session.drag_leave();
        assert_eq!(
            session.drag_over_effect(
                &live,
                &icons,
                "DISP@125%",
                viewport,
                &ids,
                &offered(),
                screen(),
                7
            ),
            0
        );
        let old_bytes = read_existing(&case.path).unwrap();
        assert!(session
            .drop(
                &mut live,
                &icons,
                "DISP@125%",
                viewport,
                &ids,
                &offered(),
                screen(),
                7
            )
            .is_err());
        assert_eq!(old_bytes, read_existing(&case.path).unwrap());

        session
            .drag_enter(
                &case.path,
                &live,
                &icons,
                "DISP@125%",
                viewport,
                &offered(),
                &ids,
                4,
                3,
            )
            .unwrap();
        assert!(session
            .drop(
                &mut live,
                &icons,
                "DISP@125%",
                viewport,
                &ids,
                &offered(),
                screen(),
                2
            )
            .is_err()); // source offered MOVE only
        assert_eq!(old_bytes, read_existing(&case.path).unwrap());

        session
            .drag_enter(
                &case.path,
                &live,
                &icons,
                "DISP@125%",
                viewport,
                &offered(),
                &ids,
                4,
                3,
            )
            .unwrap();
        assert!(session
            .drop(
                &mut live,
                &icons,
                "DISP@125%",
                viewport,
                &ids,
                &offered(),
                screen(),
                7
            )
            .is_ok());
        assert!(session
            .drop(
                &mut live,
                &icons,
                "DISP@125%",
                viewport,
                &ids,
                &offered(),
                screen(),
                7
            )
            .is_err()); // consumed
        assert!(live.group(source).unwrap().tabs[0].items.is_empty());
        assert_eq!(live.group(target).unwrap().tabs[0].items.len(), 2);
        let persisted = State::load(&case.path).unwrap();
        assert!(persisted.group(source).unwrap().tabs[0].items.is_empty());
        assert_eq!(fs::read_dir(&case.root).unwrap().count(), 1);
    }

    #[test]
    fn persisted_drop_is_not_acknowledged_if_workspace_changes_during_hover() {
        let (mut live, icons, viewport, source, target) = fixture();
        let case = TempCase::new(&live);
        let ids = [source, target];
        let mut session = NativeOlePersistedSession::default();
        session
            .drag_enter(
                &case.path,
                &live,
                &icons,
                "DISP@125%",
                viewport,
                &offered(),
                &ids,
                4,
                3,
            )
            .unwrap();
        assert_eq!(
            session.drag_over_effect(
                &live,
                &icons,
                "DISP@125%",
                viewport,
                &ids,
                &offered(),
                screen(),
                7
            ),
            4
        );
        let previous = serialized_state(&live).unwrap();
        let mut outside = live.clone();
        outside.tint = !outside.tint;
        fs::write(&case.path, serde_json::to_vec_pretty(&outside).unwrap()).unwrap();
        let changed = read_existing(&case.path).unwrap();
        assert!(session
            .drop(
                &mut live,
                &icons,
                "DISP@125%",
                viewport,
                &ids,
                &offered(),
                screen(),
                7
            )
            .is_err());
        assert_eq!(read_existing(&case.path).unwrap(), changed);
        assert_eq!(serialized_state(&live).unwrap(), previous);
        assert!(
            session.drag_over_effect(
                &live,
                &icons,
                "DISP@125%",
                viewport,
                &ids,
                &offered(),
                screen(),
                7
            ) == 0
        );
        assert_eq!(fs::read_dir(&case.root).unwrap().count(), 1);
    }

    #[test]
    fn persisted_ole_session_rejects_valid_but_substituted_drop_paths() {
        let (mut live, icons, viewport, source, target) = fixture();
        let case = TempCase::new(&live);
        let ids = [source, target];
        let mut session = NativeOlePersistedSession::default();
        session
            .drag_enter(
                &case.path,
                &live,
                &icons,
                "DISP@125%",
                viewport,
                &offered(),
                &ids,
                4,
                3,
            )
            .unwrap();
        let substitute = [PathBuf::from("C:/Lab/C.lnk")];
        assert_eq!(
            session.drag_over_effect(
                &live,
                &icons,
                "DISP@125%",
                viewport,
                &ids,
                &substitute,
                screen(),
                7
            ),
            0
        );
        let before = read_existing(&case.path).unwrap();
        assert!(session
            .drop(
                &mut live,
                &icons,
                "DISP@125%",
                viewport,
                &ids,
                &substitute,
                screen(),
                7
            )
            .is_err());
        assert_eq!(read_existing(&case.path).unwrap(), before);
    }

    #[test]
    fn invalid_ole_effect_geometry_and_double_commit_never_stage_files() {
        let (mut state, icons, v, source, target) = fixture();
        let t = TempCase::new(&state);
        let original = read_existing(&t.path).unwrap();
        let first = begin(&t, &state, &icons, v, source, target);
        assert!(first
            .commit(
                &mut state,
                &icons,
                "DISP@125%",
                v,
                &[source, target],
                &offered(),
                screen(),
                2
            )
            .is_err());
        let second = begin(&t, &state, &icons, v, source, target);
        assert!(second
            .commit(
                &mut state,
                &icons,
                "DISP@125%",
                v,
                &[source, target],
                &offered(),
                (-10000, 9000),
                7
            )
            .is_err());
        assert_eq!(read_existing(&t.path).unwrap(), original);
        assert_eq!(fs::read_dir(&t.root).unwrap().count(), 1);
    }
}
