use super::*;

impl LiveLayoutTransaction {
    pub(super) fn persist_verified_generation(
        &self,
        positions: &[ViewItem],
        state: &State,
        allow_inventory_change: bool,
    ) -> Result<(), String> {
        if positions.len()!=self.original.len() {
            return Err("Cannot checkpoint an incomplete native layout".into());
        }
        let bytes=std::fs::read(&self.journal).map_err(|e|e.to_string())?;
        let mut data:Value=serde_json::from_slice(&bytes)
            .map_err(|e|e.to_string())?;
        if data["format"]!=1 || data["monitor_signature"]!=self.initial_monitor_signature {
            return Err("Immutable native journal changed during layout".into());
        }
        if !allow_inventory_change
            && data["original_icons"].as_array().is_none_or(|a|a.len()!=self.original.len())
        {
            return Err("Immutable native journal original icon count changed".into());
        }
        data["last_ack_generation"]=json!(self.count_changes+1);
        data["auto_restore_requires_exact_ack"]=json!(true);
        data["snap_to_grid"]=json!(self.snap_to_grid);
        data["original_snap_to_grid"]=json!(self.original_snap_to_grid);
        data["hidden_park_point"]=match self.hidden_park {
            Some((x, y)) => json!({"x": x, "y": y}),
            None => Value::Null,
        };
        data["nudged_loose_positions"]=json!(self.nudged_loose.iter().map(|(path, (x, y))| json!({
            "path":path,"x":x,"y":y
        })).collect::<Vec<_>>());
        // Loose positions can change before an item is pinned. Persist its
        // latest Explorer baseline with the first generation that moves it.
        data["original_icons"]=json!(self.original.iter().map(|icon|json!({
            "path":icon.path,"label":icon.label,"x":icon.pos.0,"y":icon.pos.1
        })).collect::<Vec<_>>());
        data["allowed_box_paths"]=json!(self.moved.iter().map(|(path, _)| path.to_lowercase())
            .collect::<BTreeSet<_>>());
        let coordinates = DesktopCoordinates::current()?;
        data["allowed_box_bounds"]=json!(state.groups.iter().map(|group| {
            let rect = coordinates.screen_rect(group.bounds);
            json!({"id":group.id,"x":rect.x,"y":rect.y,"w":rect.w,"h":rect.h})
        }).collect::<Vec<_>>());
        data["last_verified_positions"]=json!(positions.iter().map(|icon|json!({
            "path":icon.path,"label":icon.label,
            "x":icon.pos.0,"y":icon.pos.1
        })).collect::<Vec<_>>());
        let encoded=serde_json::to_vec_pretty(&data).map_err(|e|e.to_string())?;
        replace_verified_journal(&self.journal,&encoded)
    }

}
