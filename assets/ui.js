const elements = {
  tabs: document.getElementById("tabs"),
  address: document.getElementById("address"),
  go: document.getElementById("go"),
  newTab: document.getElementById("new-tab"),
  home: document.getElementById("home"),
  navBack: document.getElementById("nav-back"),
  navForward: document.getElementById("nav-forward"),
  navReload: document.getElementById("nav-reload"),
  sidebarToggle: document.getElementById("rail-toggle"),
  brandIcon: document.getElementById("brand-icon"),
  tabSearch: document.getElementById("tab-search"),
  paletteBackdrop: document.getElementById("palette-backdrop"),
  palette: document.getElementById("palette"),
  paletteInput: document.getElementById("palette-input"),
  paletteResults: document.getElementById("palette-results"),
  tabMenu: document.getElementById("tab-menu"),
};

const state = {
  flatTabs: [],
  activeTabId: null,
  sidebarCollapsed: false,
  paletteIndex: 0,
  peekEnabled: false,
  peeking: false,
  defaultFavicon: null,
  tabMap: new Map(),
  tabQuery: "",
  lastTabs: [],
  lastActive: null,
};

const menuState = {
  open: false,
  tabId: null,
};

const bridge = window.webkit?.messageHandlers?.owl;

const send = (type, payload = {}) => {
  if (!bridge) {
    console.warn(`Bridge not available. Attempted to send: ${type}`, payload);
    return;
  }

  try {
    bridge.postMessage(JSON.stringify({ type, payload }));
  } catch (error) {
    console.error(`Failed to send message: ${type}`, error);
  }
};

const createActionButton = ({ className = "ghost", text, title, ariaLabel, onClick }) => {
  const button = document.createElement("button");
  button.className = className;
  button.textContent = text;
  button.title = title;
  button.setAttribute("aria-label", ariaLabel || title);

  button.addEventListener("click", (event) => {
    event.stopPropagation();
    onClick();
  });

  return button;
};

const buildRow = (node, depth) => {
  const row = document.createElement("div");
  row.className = "tab";

  if (node.is_group) {
    row.classList.add("is-group");
  }

  row.style.setProperty("--depth", depth);

  Object.assign(row.dataset, {
    id: node.id,
    hasChildren: String(Boolean(node.children?.length)),
    pinned: String(Boolean(node.is_pinned)),
    muted: String(Boolean(node.is_muted)),
    suspended: String(Boolean(node.is_suspended)),
  });

  row.setAttribute("role", "treeitem");
  row.setAttribute("aria-selected", String(Boolean(node.is_active)));
  row.tabIndex = node.is_active ? 0 : -1;

  if (node.children?.length) {
    row.setAttribute("aria-expanded", String(Boolean(node.is_expanded)));
  }

  const fullTitle = node.title || node.url || "Tab";
  row.dataset.fullTitle = fullTitle;
  if (!state.sidebarCollapsed) {
    row.title = fullTitle;
  }

  const expander = createActionButton({
    className: "expander",
    text: "",
    title: "Toggle group",
    onClick: () => send("tab.toggle", { id: node.id }),
  });

  const iconWrap = document.createElement("div");
  iconWrap.className = "tab-icon";
  iconWrap.setAttribute("aria-hidden", "true");

  const icon = document.createElement("img");
  icon.className = "tab-favicon";
  icon.alt = "";
  icon.decoding = "async";

  const favicon = node.favicon_uri || state.defaultFavicon || "";
  if (favicon) {
    icon.src = favicon;
  }
  row.dataset.favicon = node.favicon_uri || "";

  iconWrap.appendChild(icon);

  const meta = document.createElement("div");
  meta.className = "tab-meta";

  const title = document.createElement("div");
  title.className = "tab-title";
  title.textContent = node.title || "Untitled";

  meta.append(title);

  const actions = document.createElement("div");
  actions.className = "tab-actions";

  const closeBtn = createActionButton({
    text: "×",
    title: "Close tab",
    onClick: () => send("tab.close", { id: node.id }),
  });

  actions.append(closeBtn);
  row.append(expander, iconWrap, meta, actions);

  row.addEventListener("click", () => send("tab.select", { id: node.id }));
  row.addEventListener("contextmenu", (event) => {
    event.preventDefault();
    event.stopPropagation();
    openTabMenu(event, node.id);
  });
  row.addEventListener("keydown", (event) => {
    if (event.shiftKey && event.key === "F10") {
      event.preventDefault();
      openTabMenu(event, node.id);
    }
    if (event.key === "ContextMenu") {
      event.preventDefault();
      openTabMenu(event, node.id);
    }
  });

  return row;
};

const renderNodes = (nodes, depth, fragment) => {
  for (const node of nodes) {
    fragment.appendChild(buildRow(node, depth));
    state.flatTabs.push(node.id);
    state.tabMap.set(node.id, node);

    if (node.children?.length && node.is_expanded) {
      renderNodes(node.children, depth + 1, fragment);
    }
  }
};

const renderTabs = (nodes) => {
  state.flatTabs = [];
  state.tabMap = new Map();
  const fragment = document.createDocumentFragment();
  renderNodes(nodes, 0, fragment);

  if (elements.tabs) {
    elements.tabs.textContent = "";
    elements.tabs.appendChild(fragment);
  }

  syncCollapsedTabTitles(state.sidebarCollapsed);
};

const findActive = (nodes, id) => {
  for (const node of nodes) {
    if (node.id === id) return node;

    const found = findActive(node.children || [], id);
    if (found) return found;
  }
  return null;
};

const filterNodes = (nodes, query) => {
  if (!query) return nodes;
  const needle = query.toLowerCase();
  const filtered = [];

  for (const node of nodes) {
    const title = node.title?.toLowerCase() || "";
    const url = node.url?.toLowerCase() || "";
    const matches = title.includes(needle) || url.includes(needle);
    const children = node.children?.length ? filterNodes(node.children, query) : [];

    if (matches || children.length) {
      filtered.push({
        ...node,
        children,
        is_expanded: true,
      });
    }
  }

  return filtered;
};

const updateTreeFavicon = (nodes, ids, faviconUri) => {
  for (const node of nodes) {
    if (ids.has(node.id)) {
      node.favicon_uri = faviconUri;
    }
    if (node.children?.length) {
      updateTreeFavicon(node.children, ids, faviconUri);
    }
  }
};

const applyState = (newState) => {
  state.activeTabId = newState.active;
  state.lastTabs = newState.tabs;
  state.lastActive = newState.active;

  const filtered = filterNodes(state.lastTabs, state.tabQuery);
  renderTabs(filtered);
  closeTabMenu();

  const activeNode = state.lastActive ? findActive(state.lastTabs, state.lastActive) : null;

  if (activeNode && elements.address) {
    elements.address.value = activeNode.url || "";
  }
};

const applyNavState = (nav) => {
  if (elements.navBack) {
    elements.navBack.disabled = !nav.can_go_back;
  }

  if (elements.navForward) {
    elements.navForward.disabled = !nav.can_go_forward;
  }

  document.body.classList.toggle("is-loading", nav.is_loading);

  if (elements.navReload) {
    if (nav.is_loading) {
      elements.navReload.textContent = "Stop";
      elements.navReload.dataset.mode = "stop";
      elements.navReload.setAttribute("aria-label", "Stop");
    } else {
      elements.navReload.textContent = "Reload";
      elements.navReload.dataset.mode = "reload";
      elements.navReload.setAttribute("aria-label", "Reload");
    }
  }
};

const applyAssets = (payload) => {
  if (!payload?.default_favicon) return;

  state.defaultFavicon = payload.default_favicon;

  if (elements.brandIcon) {
    elements.brandIcon.src = state.defaultFavicon;
  }

  if (!elements.tabs) return;

  elements.tabs.querySelectorAll(".tab").forEach((row) => {
    const img = row.querySelector(".tab-favicon");
    if (!img) return;
    if (!row.dataset.favicon) {
      if (state.defaultFavicon) {
        img.src = state.defaultFavicon;
      }
    }
  });
};

const applyFaviconUpdate = (payload) => {
  if (!payload?.ids?.length) return;

  const ids = new Set(payload.ids);
  updateTreeFavicon(state.lastTabs, ids, payload.favicon_uri || null);

  payload.ids.forEach((id) => {
    const node = state.tabMap.get(id);
    if (node) {
      node.favicon_uri = payload.favicon_uri || null;
    }

    const row = elements.tabs?.querySelector(`.tab[data-id="${id}"]`);
    const img = row?.querySelector(".tab-favicon");
    if (!img) return;

    row.dataset.favicon = payload.favicon_uri || "";
    const fallback = payload.favicon_uri || state.defaultFavicon || "";
    if (fallback) {
      img.src = fallback;
    }
  });
};

const syncCollapsedTabTitles = (collapsed) => {
  if (!elements.tabs) return;

  elements.tabs.querySelectorAll(".tab").forEach((row) => {
    if (collapsed) {
      row.removeAttribute("title");
      return;
    }

    const fullTitle = row.dataset.fullTitle;
    if (fullTitle) {
      row.title = fullTitle;
    }
  });
};

const applySidebarState = (collapsed) => {
  state.sidebarCollapsed = collapsed;
  document.body.classList.toggle("sidebar-collapsed", collapsed);

  elements.sidebarToggle?.setAttribute(
    "aria-label",
    collapsed ? "Expand sidebar" : "Collapse sidebar"
  );

  syncCollapsedTabTitles(collapsed);
};

const setSidebarCollapsed = (collapsed) => {
  applySidebarState(collapsed);
  send("ui.sidebar.toggle", { collapsed });
};

const navigateFromAddress = () => {
  const value = elements.address?.value.trim();
  if (value) {
    send("nav.go", { url: value });
  }
};

const COMMANDS = [
  { id: "new-tab", label: "New Tab", run: () => send("tab.create") },
  { id: "back", label: "Go Back", run: () => send("nav.back") },
  { id: "forward", label: "Go Forward", run: () => send("nav.forward") },
  { id: "reload", label: "Reload", run: () => send("nav.reload") },
  { id: "home", label: "Go Home", run: () => send("nav.home") },
  {
    id: "toggle-sidebar",
    label: "Toggle Sidebar",
    run: () => setSidebarCollapsed(!state.sidebarCollapsed)
  },
  {
    id: "focus-address",
    label: "Focus Address Bar",
    run: () => {
      elements.address?.focus();
      elements.address?.select();
    }
  },
];

const renderPaletteResults = (query) => {
  const needle = query.trim().toLowerCase();
  const results = COMMANDS.filter(({ label }) => label.toLowerCase().includes(needle));

  if (!elements.paletteResults) return results;

  elements.paletteResults.textContent = "";

  results.forEach((cmd, index) => {
    const item = document.createElement("div");
    item.className = "palette-item";

    if (index === state.paletteIndex) {
      item.classList.add("is-active");
    }

    item.textContent = cmd.label;
    item.addEventListener("click", () => {
      cmd.run();
      closePalette();
    });

    elements.paletteResults.appendChild(item);
  });

  return results;
};

const openPalette = () => {
  document.body.classList.add("palette-open");

  if (elements.paletteInput) {
    elements.paletteInput.value = "";
    state.paletteIndex = 0;
    renderPaletteResults("");
    elements.paletteInput.focus();
  }
};

const closePalette = () => {
  document.body.classList.remove("palette-open");
  elements.paletteInput?.blur();
};

const closeTabMenu = () => {
  if (!elements.tabMenu) return;
  elements.tabMenu.classList.remove("is-open");
  elements.tabMenu.setAttribute("aria-hidden", "true");
  menuState.open = false;
  menuState.tabId = null;
};

const openTabMenu = (event, id) => {
  if (!elements.tabMenu) return;

  const node = state.tabMap.get(id);
  if (!node) return;

  const pinItem = elements.tabMenu.querySelector('[data-action="pin"]');
  const muteItem = elements.tabMenu.querySelector('[data-action="mute"]');
  const unloadItem = elements.tabMenu.querySelector('[data-action="unload"]');

  if (pinItem) {
    pinItem.textContent = node.is_pinned ? "Unpin" : "Pin";
  }
  if (muteItem) {
    muteItem.textContent = node.is_muted ? "Unmute" : "Mute";
  }
  if (unloadItem) {
    unloadItem.textContent = node.is_suspended ? "Reload" : "Unload";
  }

  closeTabMenu();
  menuState.open = true;
  menuState.tabId = id;

  const rect = event.currentTarget?.getBoundingClientRect?.();
  const x = event.clientX || rect?.right || 0;
  const y = event.clientY || rect?.top || 0;

  elements.tabMenu.style.left = `${x}px`;
  elements.tabMenu.style.top = `${y}px`;
  elements.tabMenu.classList.add("is-open");
  elements.tabMenu.setAttribute("aria-hidden", "false");

  requestAnimationFrame(() => {
    const menuRect = elements.tabMenu.getBoundingClientRect();
    const clampedX = Math.min(x, window.innerWidth - menuRect.width - 8);
    const clampedY = Math.min(y, window.innerHeight - menuRect.height - 8);
    elements.tabMenu.style.left = `${Math.max(8, clampedX)}px`;
    elements.tabMenu.style.top = `${Math.max(8, clampedY)}px`;
    pinItem?.focus();
  });
};

const setupEventHandlers = () => {
  elements.address?.addEventListener("keydown", ({ key }) => {
    if (key === "Enter") navigateFromAddress();
  });

  elements.go?.addEventListener("click", navigateFromAddress);

  if (elements.navBack) {
    elements.navBack.textContent = "Back";
    elements.navBack.setAttribute("aria-label", "Back");
  }
  if (elements.navForward) {
    elements.navForward.textContent = "Forward";
    elements.navForward.setAttribute("aria-label", "Forward");
  }
  if (elements.navReload) {
    elements.navReload.textContent = "Reload";
    elements.navReload.dataset.mode = "reload";
    elements.navReload.setAttribute("aria-label", "Reload");
  }

  elements.newTab?.addEventListener("click", () => send("tab.create"));
  elements.home?.addEventListener("click", () => send("nav.home"));
  elements.navBack?.addEventListener("click", () => send("nav.back"));
  elements.navForward?.addEventListener("click", () => send("nav.forward"));

  elements.navReload?.addEventListener("click", () => {
    const mode = elements.navReload.dataset.mode;
    send(mode === "stop" ? "nav.stop" : "nav.reload");
  });

  elements.sidebarToggle?.addEventListener("click", () =>
    setSidebarCollapsed(!state.sidebarCollapsed)
  );

  elements.tabSearch?.addEventListener("input", (event) => {
    state.tabQuery = event.target.value.trim();
    const filtered = filterNodes(state.lastTabs, state.tabQuery);
    renderTabs(filtered);
  });

  if (state.peekEnabled) {
    window.addEventListener("keydown", ({ altKey }) => {
      if (altKey && state.sidebarCollapsed && !state.peeking) {
        state.peeking = true;
        setSidebarCollapsed(false);
      }
    });

    window.addEventListener("keyup", ({ altKey }) => {
      if (state.peeking && !altKey) {
        state.peeking = false;
        setSidebarCollapsed(true);
      }
    });
  }

  elements.paletteBackdrop?.addEventListener("click", closePalette);

  elements.paletteInput?.addEventListener("input", ({ target }) => {
    state.paletteIndex = 0;
    renderPaletteResults(target.value);
  });

  elements.paletteInput?.addEventListener("keydown", (event) => {
    const results = renderPaletteResults(elements.paletteInput.value);

    const keyHandlers = {
      ArrowDown: () => {
        event.preventDefault();
        state.paletteIndex = Math.min(state.paletteIndex + 1, results.length - 1);
        renderPaletteResults(elements.paletteInput.value);
      },
      ArrowUp: () => {
        event.preventDefault();
        state.paletteIndex = Math.max(state.paletteIndex - 1, 0);
        renderPaletteResults(elements.paletteInput.value);
      },
      Enter: () => {
        event.preventDefault();
        if (results[state.paletteIndex]) {
          results[state.paletteIndex].run();
          closePalette();
        } else {
          const trimmedValue = elements.paletteInput.value.trim();
          if (trimmedValue) {
            send("nav.go", { url: trimmedValue });
            closePalette();
          }
        }
      },
      Escape: () => {
        event.preventDefault();
        closePalette();
      },
    };

    keyHandlers[event.key]?.();
  });

  window.addEventListener("keydown", (event) => {
    const modKey = event.ctrlKey || event.metaKey;
    const key = event.key.toLowerCase();

    const shortcuts = {
      l: () => {
        event.preventDefault();
        elements.address?.focus();
        elements.address?.select();
      },
      k: () => {
        event.preventDefault();
        openPalette();
      },
      p: () => {
        event.preventDefault();
        openPalette();
      },
    };

    if (modKey && shortcuts[key]) {
      shortcuts[key]();
    }

    if (event.key === "Escape") {
      if (document.body.classList.contains("palette-open")) {
        closePalette();
      }
      if (menuState.open) {
        closeTabMenu();
      }
    }
  });

  elements.tabMenu?.addEventListener("click", (event) => {
    const button = event.target.closest("button[data-action]");
    if (!button || menuState.tabId == null) return;

    const action = button.dataset.action;
    if (action === "pin") send("tab.pin", { id: menuState.tabId });
    if (action === "mute") send("tab.mute", { id: menuState.tabId });
    if (action === "unload") send("tab.unload", { id: menuState.tabId });

    closeTabMenu();
  });

  elements.tabMenu?.addEventListener("keydown", (event) => {
    const items = Array.from(
      elements.tabMenu.querySelectorAll("button[data-action]")
    );
    if (!items.length) return;

    const currentIndex = items.indexOf(document.activeElement);

    if (event.key === "ArrowDown") {
      event.preventDefault();
      const next = items[Math.min(currentIndex + 1, items.length - 1)];
      next?.focus();
    }

    if (event.key === "ArrowUp") {
      event.preventDefault();
      const prev = items[Math.max(currentIndex - 1, 0)];
      prev?.focus();
    }

    if (event.key === "Enter" && document.activeElement?.dataset?.action) {
      document.activeElement.click();
    }
  });

  window.addEventListener("click", (event) => {
    if (!menuState.open) return;
    if (!elements.tabMenu?.contains(event.target)) {
      closeTabMenu();
    }
  });

  window.addEventListener("contextmenu", (event) => {
    if (menuState.open && !elements.tabMenu?.contains(event.target)) {
      closeTabMenu();
    }
  });

  elements.tabs?.addEventListener("keydown", (event) => {
    if (!state.flatTabs.length) return;

    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();

      const currentIndex = state.flatTabs.indexOf(state.activeTabId);
      const delta = event.key === "ArrowDown" ? 1 : -1;
      const nextIndex =
        currentIndex === -1
          ? 0
          : Math.min(Math.max(currentIndex + delta, 0), state.flatTabs.length - 1);

      const nextId = state.flatTabs[nextIndex];
      if (nextId != null) {
        send("tab.select", { id: nextId });
      }
    }
  });
};

window.__owl_receive = (message) => {
  if (!message?.type) return;

  const messageHandlers = {
    "state.tabs": () => applyState(message.payload),
    "state.nav": () => applyNavState(message.payload),
    "state.assets": () => applyAssets(message.payload),
    "state.favicon": () => applyFaviconUpdate(message.payload),
    "state.sidebar": () => applySidebarState(Boolean(message.payload?.collapsed)),
  };

  messageHandlers[message.type]?.();
};

window.addEventListener("DOMContentLoaded", () => {
  setupEventHandlers();
  send("ui.ready");
});
