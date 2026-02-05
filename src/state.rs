use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct TabNode {
    pub id: u64,
    pub parent: Option<u64>,
    pub title: String,
    pub url: String,
    pub favicon_uri: Option<String>,
    pub children: Vec<u64>,
    pub is_expanded: bool,
    pub is_pinned: bool,
    pub is_muted: bool,
    pub is_suspended: bool,
    pub is_group: bool,
}

#[derive(Debug, Clone)]
pub struct BrowserState {
    next_id: u64,
    pub tabs: HashMap<u64, TabNode>,
    pub roots: Vec<u64>,
    pub active: Option<u64>,
    pub recently_closed: Vec<ClosedTab>,
}

#[derive(Debug, Serialize)]
pub struct UiTabNode {
    pub id: u64,
    pub title: String,
    pub url: String,
    pub favicon_uri: Option<String>,
    pub is_expanded: bool,
    pub is_active: bool,
    pub is_pinned: bool,
    pub is_muted: bool,
    pub is_suspended: bool,
    pub is_group: bool,
    pub children: Vec<UiTabNode>,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct ClosedTab {
    pub title: String,
    pub url: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct SessionState {
    pub name: String,
    pub tabs: Vec<SessionTab>,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct SessionTab {
    pub title: String,
    pub url: String,
    pub is_pinned: bool,
}

impl BrowserState {
    pub fn new() -> Self {
        let mut state = Self {
            next_id: 1,
            tabs: HashMap::new(),
            roots: Vec::new(),
            active: None,
            recently_closed: Vec::new(),
        };

        let _home_id = state.create_tab(None, "Home", "owl://home");
        let group_id = state.create_group("Reading");
        if let Some(group) = state.tabs.get_mut(&group_id) {
            group.is_expanded = true;
        }

        let webkit_id = state.create_tab(Some(group_id), "WebKitGTK", "https://webkitgtk.org");
        state.create_tab(Some(group_id), "GNOME", "https://www.gnome.org");
        state.create_tab(Some(group_id), "Fedora", "https://fedoraproject.org");

        state.set_active(webkit_id);
        state
    }

    pub fn create_tab(&mut self, parent: Option<u64>, title: &str, url: &str) -> u64 {
        self.create_tab_internal(parent, title, url, false)
    }

    pub fn create_group(&mut self, title: &str) -> u64 {
        self.create_tab_internal(None, title, "owl://group", true)
    }

    fn create_tab_internal(
        &mut self,
        parent: Option<u64>,
        title: &str,
        url: &str,
        is_group: bool,
    ) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let node = TabNode {
            id,
            parent,
            title: title.to_string(),
            url: url.to_string(),
            favicon_uri: None,
            children: Vec::new(),
            is_expanded: true,
            is_pinned: false,
            is_muted: false,
            is_suspended: false,
            is_group,
        };
        self.tabs.insert(id, node);

        if let Some(parent_id) = parent {
            if let Some(parent_node) = self.tabs.get_mut(&parent_id) {
                parent_node.children.push(id);
            }
        } else {
            self.roots.push(id);
        }

        id
    }

    pub fn remove_tab(&mut self, id: u64) {
        if let Some(node) = self.tabs.get(&id).cloned() {
            for child in node.children.clone() {
                self.remove_tab(child);
            }

            if let Some(parent_id) = node.parent {
                if let Some(parent_node) = self.tabs.get_mut(&parent_id) {
                    parent_node.children.retain(|child_id| *child_id != id);
                }
            } else {
                self.roots.retain(|root_id| *root_id != id);
            }

            self.tabs.remove(&id);
            self.recently_closed.push(ClosedTab {
                title: node.title,
                url: node.url,
            });
            if self.active == Some(id) {
                self.active = self.roots.first().copied();
            }
        }
    }

    pub fn set_active(&mut self, id: u64) {
        if self.tabs.contains_key(&id) {
            self.active = Some(id);
        }
    }

    pub fn toggle_expanded(&mut self, id: u64) {
        if let Some(node) = self.tabs.get_mut(&id) {
            if !node.children.is_empty() {
                node.is_expanded = !node.is_expanded;
            }
        }
    }

    pub fn update_tab(&mut self, id: u64, title: Option<&str>, url: Option<&str>) {
        if let Some(node) = self.tabs.get_mut(&id) {
            if let Some(title) = title {
                node.title = title.to_string();
            }
            if let Some(url) = url {
                node.url = url.to_string();
                node.favicon_uri = None;
            }
        }
    }

    pub fn toggle_pin(&mut self, id: u64) {
        if let Some(node) = self.tabs.get_mut(&id) {
            node.is_pinned = !node.is_pinned;
        }
    }

    pub fn toggle_mute(&mut self, id: u64) {
        if let Some(node) = self.tabs.get_mut(&id) {
            node.is_muted = !node.is_muted;
        }
    }

    pub fn toggle_suspended(&mut self, id: u64) {
        if let Some(node) = self.tabs.get_mut(&id) {
            node.is_suspended = !node.is_suspended;
        }
    }

    pub fn set_favicon_for_url(&mut self, url: &str, favicon_uri: Option<String>) -> Vec<u64> {
        let mut updated = Vec::new();
        for (id, node) in self.tabs.iter_mut() {
            if node.url == url {
                node.favicon_uri = favicon_uri.clone();
                updated.push(*id);
            }
        }
        updated
    }

    fn ordered_children(&self, ids: &[u64]) -> Vec<u64> {
        let mut pinned = Vec::new();
        let mut normal = Vec::new();
        for id in ids {
            if let Some(node) = self.tabs.get(id) {
                if node.is_pinned {
                    pinned.push(*id);
                } else {
                    normal.push(*id);
                }
            }
        }
        pinned.extend(normal);
        pinned
    }

    pub fn to_ui_tree(&self) -> Vec<UiTabNode> {
        fn build_node(state: &BrowserState, id: u64) -> UiTabNode {
            let node = state.tabs.get(&id).expect("tab node exists");
            let children = state
                .ordered_children(&node.children)
                .iter()
                .map(|child_id| build_node(state, *child_id))
                .collect();

            UiTabNode {
                id: node.id,
                title: node.title.clone(),
                url: node.url.clone(),
                favicon_uri: node.favicon_uri.clone(),
                is_expanded: node.is_expanded,
                is_active: state.active == Some(node.id),
                is_pinned: node.is_pinned,
                is_muted: node.is_muted,
                is_suspended: node.is_suspended,
                is_group: node.is_group,
                children,
            }
        }

        self.ordered_children(&self.roots)
            .iter()
            .map(|id| build_node(self, *id))
            .collect()
    }
}
