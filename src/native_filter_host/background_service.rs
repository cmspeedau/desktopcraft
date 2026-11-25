use super::*;

impl NativeTwoBoxBackgroundService{
    pub fn start(workspace:std::path::PathBuf,dll:std::path::PathBuf,overlay_hwnd:usize)->Self{
        let (commands,receiver)=mpsc::channel();
        let pointer_wake = commands.clone();
        let (drop_tx,drop_rx)=mpsc::sync_channel(1);
        let (page_tx,pages)=mpsc::channel();
        let (header_drops, header_rx) = mpsc::sync_channel(1);
        let stop=Arc::new(AtomicBool::new(false));
        let ready=Arc::new(AtomicBool::new(false));
        let stop_worker=Arc::clone(&stop);
        let ready_worker=Arc::clone(&ready);
        let worker=std::thread::spawn(move||{
            // A Rust worker thread does NOT inherit main UI thread DPI
            // awareness; without this, 125%-DPI monitor cells are 2048
            // logical instead of 2560 physical and icon rollback is unsafe.
            #[link(name="user32")]
            unsafe extern "system" {
                fn SetThreadDpiAwarenessContext(context:isize)->isize;
            }
            if unsafe{SetThreadDpiAwarenessContext(-4)}==0{
                return json!({"started":false,
                    "reason":"Native worker cannot use physical per-monitor DPI coordinates"});
            }
            crate::shell::init();
            run_native_hook_trial_with_event(900,&workspace,&dll,None,None,None,None,
                None,0,Some(ready_worker),false,None,Some(stop_worker),false,
                Some(usize::MAX-1),Some(NativeUiChannel{
                    commands:receiver,pointer_wake,pages:page_tx,header_drops:header_rx,overlay_hwnd,
                }),Some(drop_tx))
        });
        Self{commands,stop,ready,worker:Some(worker),real_drop_events:drop_rx,pages,header_drops}
    }

    pub(crate) fn drop_onto_tab(
        &self, destination: (u64, u64), screen: (i32, i32), paths: Vec<String>,
    ) -> Result<Receiver<Result<(), String>>, String> {
        if !self.is_ready() {
            return Err("Explorer Desktop is still loading".into());
        }
        let (result, reply) = mpsc::channel();
        self.header_drops.try_send(NativeHeaderDrop { destination, screen, paths, result: Some(result) })
            .map_err(|_| "Explorer is still handling a previous Tab drop".to_string())?;
        let _ = self.commands.send(NativeWorkerMessage::PointerEdge);
        Ok(reply)
    }

    fn send_command(&self, command: NativeTwoBoxCommand) -> Result<(), mpsc::SendError<NativeWorkerMessage>> {
        self.commands.send(NativeWorkerMessage::Command(command))
    }
    pub fn request_rule_sort(
        &self, rules: Vec<crate::model::Rule>, next_id: u64, include_existing_pins: bool,
    ) -> Result<PendingNativeWorkspaceSync, String>
    {
        if !self.is_ready() {
            return Err("Windows Explorer is still loading".into());
        }
        let (result, ack) = mpsc::channel();
        self.send_command(NativeTwoBoxCommand {
            drag_preview: None, group_id: 0, tab: 0, page_delta: 0, desktop_page: None,
            workspace_state: None, rolled_overrides: Vec::new(), validated_drop_sequence: None,
            pin_batch: Some(NativePinBatchCommand::RuleSort { rules, next_id, include_existing_pins }),
            result,
        }).map_err(|_| "Explorer rule sorting channel closed".to_string())?;
        Ok(PendingNativeWorkspaceSync { ack, issued_at: std::time::Instant::now() })
    }
    pub fn request_layout_restore(
        &self, target: State, next_id: u64,
    ) -> Result<PendingNativeWorkspaceSync, String>
    {
        if !self.is_ready() {
            return Err("Windows Explorer is still loading".into());
        }
        let (result, ack) = mpsc::channel();
        self.send_command(NativeTwoBoxCommand {
            drag_preview: None, group_id: 0, tab: 0, page_delta: 0, desktop_page: None,
            workspace_state: None, rolled_overrides: Vec::new(), validated_drop_sequence: None,
            pin_batch: Some(NativePinBatchCommand::LayoutRestore { target, next_id }),
            result,
        }).map_err(|_| "Explorer layout restore channel closed".to_string())?;
        Ok(PendingNativeWorkspaceSync { ack, issued_at: std::time::Instant::now() })
    }
    pub fn request_pin_paths(
        &self,
        group_id: u64,
        tab_id: u64,
        paths: Vec<String>,
        next_id: u64,
    ) -> Result<PendingNativeWorkspaceSync, String> {
        if !self.is_ready() {
            return Err("Windows Explorer is still loading".into());
        }
        let (result, ack) = mpsc::channel();
        self.send_command(NativeTwoBoxCommand {
            drag_preview: None,
            group_id,
            tab: 0,
            page_delta: 0,
            desktop_page: None,
            workspace_state: None,
            rolled_overrides: Vec::new(),
            validated_drop_sequence: None,
            pin_batch: Some(NativePinBatchCommand::PinPaths {
                group_id,
                tab_id,
                paths,
                next_id,
            }),
            result,
        })
        .map_err(|_| "Explorer pin-paths channel closed".to_string())?;
        Ok(PendingNativeWorkspaceSync {
            ack,
            issued_at: std::time::Instant::now(),
        })
    }
    pub fn take_layout_sync(&self) -> Option<NativeLayoutSync> {
        let mut combined: Option<NativeLayoutSync> = None;
        for msg in self.pages.try_iter() {
            match &mut combined {
                Some(prev) => {
                    if msg.pages.is_some() {
                        prev.pages = msg.pages;
                    }
                    if msg.ghost_icons.is_some() {
                        prev.ghost_icons = msg.ghost_icons;
                    }
                    if msg.visible_loose_rects.is_some() {
                        prev.visible_loose_rects = msg.visible_loose_rects;
                    }
                    prev.main_cell = msg.main_cell;
                    prev.icon_size = msg.icon_size;
                    prev.snap_to_grid = msg.snap_to_grid;
                }
                None => combined = Some(msg),
            }
        }
        combined
    }

    /// Watch a verified drop that the worker has already queued. The UI
    /// still owns the Workspace commit after the layout ACK.
    pub fn poll_verified_drop(&mut self)
        ->Result<Option<(NativeVerifiedDrop, PendingNativeWorkspaceSync)>,String>{
        if !self.is_ready(){return Err("Native drag worker no longer ready".into());}
        match self.real_drop_events.try_recv(){
            Ok(event)=>Ok(Some(event)),
            Err(mpsc::TryRecvError::Empty)=>Ok(None),
            Err(mpsc::TryRecvError::Disconnected)=>
                Err("Native Explorer drag event channel disconnected".into()),
        }
    }
    /// Box drag starts: Explorer hides that Box's icons; the reply lists them
    /// so the UI can draw copies that follow the Box. Nothing moves.
    pub fn begin_drag_preview(&self,group_id:u64,resizing:bool)->Result<mpsc::Receiver<NativeGhostIcons>,String>{
        if !self.is_ready(){return Err("Native Explorer worker unavailable".into());}
        let (reply,icons)=mpsc::channel();
        let (send,_)=mpsc::channel();
        self.send_command(NativeTwoBoxCommand{drag_preview:Some(DragPreviewRequest{
            group_id,active:true,resizing,reply:Some(reply)}),
            group_id,tab:0,page_delta:0,desktop_page:None,workspace_state:None,
            rolled_overrides:Vec::new(),validated_drop_sequence:None,pin_batch:None,result:send,
        }).map_err(|_|"Native drag preview channel stopped".to_string())?;
        Ok(icons)
    }
    /// Box drag ended without a layout change: show the current view again.
    pub fn end_drag_preview(&self,group_id:u64)->Result<PendingNativeWorkspaceSync,String>{
        let (send,recv)=mpsc::channel();
        self.send_command(NativeTwoBoxCommand{drag_preview:Some(DragPreviewRequest{
            group_id,active:false,resizing:false,reply:None}),
            group_id,tab:0,page_delta:0,desktop_page:None,workspace_state:None,
            rolled_overrides:Vec::new(),validated_drop_sequence:None,pin_batch:None,result:send,
        }).map_err(|_|"Native drag preview restore channel stopped".to_string())?;
        Ok(PendingNativeWorkspaceSync{ack:recv,issued_at:std::time::Instant::now()})
    }
    pub fn is_ready(&self)->bool{
        !self.stop.load(Ordering::Acquire) && self.ready.load(Ordering::Acquire)
            && self.worker.as_ref().is_some_and(|worker|!worker.is_finished())
    }
    pub fn await_ready(&self,limit:Duration)->bool{
        let start=std::time::Instant::now();
        while start.elapsed()<limit{
            if self.is_ready(){return true;}
            if self.worker.as_ref().is_none_or(|worker|worker.is_finished()){return false;}
            std::thread::sleep(Duration::from_millis(25));
        }
        self.is_ready()
    }
    pub fn selected(&self,group_id:u64,tab:usize)->Result<(),String>{
        if !self.is_ready(){return Err("Explorer two-Box native worker unavailable".into());}
        let (send,recv)=mpsc::channel();
        self.send_command(NativeTwoBoxCommand{drag_preview:None,
            group_id,tab,page_delta:0,desktop_page:None,
            workspace_state:None,rolled_overrides:Vec::new(),
            validated_drop_sequence:None,pin_batch:None,result:send})
            .map_err(|_|"Explorer two-Box command channel stopped".to_string())?;
        match recv.recv_timeout(Duration::from_secs(3)){
            Ok(result)=>result.map(|_|()),
            Err(_)=>{
                // A late ACK may never change Explorer policy behind a
                // reverted UI. Stop this entire lab filter and fail open.
                self.stop.store(true,Ordering::Release);
                Err("Native Tab ACK timed out; whole Explorer filter is stopping fail-open".into())
            }
        }
    }
    pub fn change_page(&self,group_id:u64,tab:usize,delta:i8)->Result<usize,String>{
        if !self.is_ready(){return Err("Explorer Native Box layout worker unavailable".into());}
        if delta!=1 && delta!=-1{
            return Err("Native page requires a single previous/next change".into());
        }
        let (send,recv)=mpsc::channel();
        self.send_command(NativeTwoBoxCommand{drag_preview:None,
            group_id,tab,page_delta:delta,desktop_page:None,
            workspace_state:None,rolled_overrides:Vec::new(),
            validated_drop_sequence:None,pin_batch:None,result:send})
            .map_err(|_|"Native Box paging worker channel stopped".to_string())?;
        match recv.recv_timeout(Duration::from_secs(3)){
            Ok(result)=>result,
            Err(_)=>{
                self.stop.store(true,Ordering::Release);
                Err("Native page ACK timed out; fail-open restoration started".into())
            }
        }
    }
    pub fn select_desktop_page(&self,page:usize)->Result<(),String>{
        if !self.is_ready(){return Err("Native workspace page worker unavailable".into());}
        let (send,recv)=mpsc::channel();
        self.send_command(NativeTwoBoxCommand{drag_preview:None,
            group_id:0,tab:0,page_delta:0,desktop_page:Some(page),
            workspace_state:None,rolled_overrides:Vec::new(),
            validated_drop_sequence:None,pin_batch:None,result:send
        }).map_err(|_|"Native workspace page channel stopped".to_string())?;
        match recv.recv_timeout(Duration::from_secs(3)){
            Ok(result)=>result.map(|_|()),
            Err(_)=>{
                self.stop.store(true,Ordering::Release);
                Err("Native workspace page ACK timed out; filter stopping fail-open".into())
            }
        }
    }
    /// Main UI's current group/Page/Tab/geometry changes are proposed as
    /// one atomic native generation. Source files are never moved by this.
    pub fn begin_workspace_sync(&self,next:State)
        ->Result<PendingNativeWorkspaceSync,String>{
        self.begin_workspace_sync_with_rollup_overrides(next, Vec::new())
    }
    pub fn begin_workspace_sync_with_rollup_overrides(
        &self,
        next: State,
        rolled_overrides: Vec<(u64, bool)>,
    ) -> Result<PendingNativeWorkspaceSync, String> {
        if !self.is_ready(){return Err("Actual Explorer native worker unavailable".into());}
        let (send,recv)=mpsc::channel();
        self.send_command(NativeTwoBoxCommand{drag_preview:None,
            group_id:0,tab:0,page_delta:0,desktop_page:None,
            workspace_state:Some(next),rolled_overrides,
            validated_drop_sequence:None,pin_batch:None,result:send
        }).map_err(|_|"Real Explorer Workspace channel stopped".to_string())?;
        Ok(PendingNativeWorkspaceSync{ack:recv,issued_at:std::time::Instant::now()})
    }
    pub fn begin_page_change(&self,group_id:u64,tab:usize,delta:i8)
        ->Result<PendingNativeWorkspaceSync,String>{
        if !self.is_ready(){return Err("Native Explorer page worker unavailable".into());}
        if delta==0 || delta.unsigned_abs()>MAX_SCROLL_ROWS_PER_CHANGE {
            return Err("Native row scroll must move 1 to 8 rows".into());
        }
        let (send,recv)=mpsc::channel();
        self.send_command(NativeTwoBoxCommand{drag_preview:None,
            group_id,tab,page_delta:delta,desktop_page:None,
            workspace_state:None,rolled_overrides:Vec::new(),
            validated_drop_sequence:None,pin_batch:None,result:send
        }).map_err(|_|"Native Explorer page change command channel stopped".to_string())?;
        Ok(PendingNativeWorkspaceSync{ack:recv,issued_at:std::time::Instant::now()})
    }
    pub fn poll_workspace_sync(&self,request:&PendingNativeWorkspaceSync)
        ->Result<NativeWorkspaceReply,String>{
        if self.stop.load(Ordering::Acquire){
            return Err("Native Explorer worker already stopping".into());
        }
        match request.poll(){
            Ok(_) if !self.is_ready()=>
                Err("Native Explorer worker stopped before Workspace ACK could be applied".into()),
            Ok(reply)=>Ok(reply),
            Err(reason)=>{
                self.stop.store(true,Ordering::Release);
                Err(reason)
            }
        }
    }
    pub fn shutdown(&mut self)->Value{
        self.stop.store(true,Ordering::Release);
        self.ready.store(false,Ordering::Release);
        let Some(worker)=self.worker.take()else{
            return json!({"reason":"already shut down"});
        };
        // A stalled Explorer STA must never hang the entire Windows desktop
        // UI forever on WM_CLOSE. Preserve the on-disk exact-coordinate
        // recovery journal if graceful COM rollback cannot be verified.
        // The native DLL has its own short fail-open policy lease.
        let started=std::time::Instant::now();
        while !worker.is_finished() && started.elapsed()<Duration::from_secs(4) {
            std::thread::sleep(Duration::from_millis(25));
        }
        if worker.is_finished(){
            worker.join().unwrap_or_else(|_|json!({"started":false,
                "reason":"Native worker panicked; verify manual recovery checkpoint"}))
        }else{
            let checkpoint=crate::host::data_dir().join("native-inbox-recovery.json");
            // Dropping a JoinHandle detaches a still-busy worker. The UI may
            // exit, but NO success/rollback may be claimed without a report.
            drop(worker);
            json!({
                "started":true,"graceful_shutdown_verified":false,
                "reason":"Explorer worker did not finish within bounded shutdown; manual native position recovery must be verified",
                "recovery_journal_pending":checkpoint.exists(),
                "recovery_journal_path":checkpoint,
                "native_filter_lease_must_expire_fail_open":true,
            })
        }
    }
}
impl Drop for NativeTwoBoxBackgroundService{
    fn drop(&mut self){let _=self.shutdown();}
}
