use crate::*;

impl App {
    pub(super) fn refresh_portals(&mut self, time: f64) {
        if let Some(ref worker) = self.portal_refresh {
            let mut notify_error = None;
            while let Ok((group_id, tab_id, folder, sort, result)) = worker.results.try_recv() {
                self.portal_refresh_inflight.remove(&(group_id, tab_id));
                match result {
                    Ok(items) => {
                        if let Some(g) = self.state.group_mut(group_id) {
                            if let Some(tab) = g.tabs.iter_mut()
                                .find(|t| t.id == tab_id && t.folder.as_deref() == Some(folder.as_str()))
                            {
                                tab.items = items;
                                if tab.sort != sort {
                                    tab.sort_items();
                                }
                            }
                        }
                    }
                    Err(e) => notify_error = Some(e),
                }
            }
            if let Some(e) = notify_error {
                self.notify(e, time);
            }
        }
        if time - self.last_refresh > REFRESH && !self.has_blocking_dialog() && self.drag.is_none()
        {
            self.last_refresh = time;
            if self.portal_refresh.is_none() {
                self.portal_refresh = PortalRefreshWorker::start();
            }
            if let Some(ref worker) = self.portal_refresh {
                for g in &self.state.groups {
                    for tab in &g.tabs {
                        if let Some(ref folder) = tab.folder {
                            if self.portal_refresh_inflight.insert((g.id, tab.id))
                                && worker.requests.send((g.id, tab.id, folder.clone(), tab.sort)).is_err()
                            {
                                self.portal_refresh_inflight.remove(&(g.id, tab.id));
                            }
                        }
                    }
                }
            } else {
                let mut error = None;
                for g in &mut self.state.groups {
                    for tab in &mut g.tabs {
                        if tab.folder.is_some() {
                            if let Err(e) = tab.refresh() {
                                error = Some(e)
                            }
                        }
                    }
                }
                if let Some(e) = error {
                    self.notify(e, time)
                }
            }
        }
    }
}
