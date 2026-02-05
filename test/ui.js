// Modern ES6+ rewrite with improved robustness and performance

// Centralized DOM element cache with optional chaining safety
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
    paletteBackdrop: document.getElementById("palette-backdrop"),
    palette: document.getElementById("palette"),
    paletteInput: document.getElementById("palette-input"),
    paletteResults: document.getElementById("palette-results"),
};

// Application state using a centralized object
const state = {
    flatTabs: [],
    activeTabId: null,
    sidebarCollapsed: false,
    paletteIndex: 0,
    peekEnabled: false,
    peeking: false,
};

// Bridge communication wrapper with error handling
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

// Utility: Derive icon text from node data
const deriveIconText = (node) => {
    const title = node.title?.trim();
    if (title) return title[0].toUpperCase();

    const url = node.url?.trim();
    if (!url) return "?";

    try {
        const { hostname } = new URL(url);
        return hostname[0]?.toUpperCase() || "?";
    } catch {
        return url[0]?.toUpperCase() || "?";
    }
};

// Create action button with common properties
const createActionButton = ({ className = "ghost", text, title, ariaLabel, isActive = false, onClick }) => {
    const button = document.createElement("button");
    button.className = className;
    button.textContent = text;
    button.title = title;
    button.setAttribute("aria-label", ariaLabel || title);

    if (isActive) {
        button.classList.add("is-active");
    }

    button.addEventListener("click", (event) => {
        event.stopPropagation();
        onClick();
    });

    return button;
};

// Build a single tab row
const buildRow = (node, depth) => {
    const row = document.createElement("div");
    row.className = "tab";

    if (node.is_group) {
        row.classList.add("is-group");
    }

    row.style.setProperty("--depth", depth);

    // Use dataset API for cleaner data attribute access
    Object.assign(row.dataset, {
        id: node.id,
        hasChildren: String(Boolean(node.children?.length)),
                  pinned: String(Boolean(node.is_pinned)),
                  muted: String(Boolean(node.is_muted)),
                  suspended: String(Boolean(node.is_suspended)),
    });

    // ARIA attributes
    row.setAttribute("role", "treeitem");
    row.setAttribute("aria-selected", String(Boolean(node.is_active)));

    if (node.children?.length) {
        row.setAttribute("aria-expanded", String(Boolean(node.is_expanded)));
    }

    row.title = node.title || node.url || "Tab";

    // Expander button
    const expander = createActionButton({
        className: "expander",
        text: "",
        title: "Toggle group",
        onClick: () => send("tab.toggle", { id: node.id }),
    });

    // Tab icon
    const icon = document.createElement("div");
    icon.className = "tab-icon";
    icon.textContent = deriveIconText(node);
    icon.setAttribute("aria-hidden", "true");

    // Tab metadata (title + url)
    const meta = document.createElement("div");
    meta.className = "tab-meta";

    const title = document.createElement("div");
    title.className = "tab-title";
    title.textContent = node.title || "Untitled";

    const url = document.createElement("div");
    url.className = "tab-url";
    url.textContent = node.url || "";

    meta.append(title, url);

    // Action buttons
    const actions = document.createElement("div");
    actions.className = "tab-actions";

    const pinBtn = createActionButton({
        text: "Pin",
        title: node.is_pinned ? "Unpin tab" : "Pin tab",
        isActive: node.is_pinned,
        onClick: () => send("tab.pin", { id: node.id }),
    });

    const muteBtn = createActionButton({
        text: "Mute",
        title: node.is_muted ? "Unmute tab" : "Mute tab",
        isActive: node.is_muted,
        onClick: () => send("tab.mute", { id: node.id }),
    });

    const unloadBtn = createActionButton({
        text: "Unload",
        title: node.is_suspended ? "Reload tab" : "Unload tab",
        isActive: node.is_suspended,
        onClick: () => send("tab.unload", { id: node.id }),
    });

    const closeBtn = createActionButton({
        text: "x",
        title: "Close tab",
        onClick: () => send("tab.close", { id: node.id }),
    });

    actions.append(pinBtn, muteBtn, unloadBtn, closeBtn);
    row.append(expander, icon, meta, actions);

    row.addEventListener("click", () => send("tab.select", { id: node.id }));

    return row;
};

// Recursively render tab nodes
const renderNodes = (nodes, depth, fragment) => {
    for (const node of nodes) {
        fragment.appendChild(buildRow(node, depth));
        state.flatTabs.push(node.id);

        if (node.children?.length && node.is_expanded) {
            renderNodes(node.children, depth + 1, fragment);
        }
    }
};

// Render all tabs
const renderTabs = (nodes) => {
    state.flatTabs = [];
    const fragment = document.createDocumentFragment();
    renderNodes(nodes, 0, fragment);

    if (elements.tabs) {
        elements.tabs.textContent = "";
        elements.tabs.appendChild(fragment);
    }
};

// Find active node recursively
const findActive = (nodes, id) => {
    for (const node of nodes) {
        if (node.id === id) return node;

        const found = findActive(node.children || [], id);
        if (found) return found;
    }
    return null;
};

// Apply tab state
const applyState = (newState) => {
    state.activeTabId = newState.active;
    renderTabs(newState.tabs);

    const activeNode = state.activeTabId ? findActive(newState.tabs, state.activeTabId) : null;

    if (activeNode && elements.address) {
        elements.address.value = activeNode.url || "";
    }
};

// Apply navigation state
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
        } else {
            elements.navReload.textContent = "Reload";
            elements.navReload.dataset.mode = "reload";
        }
    }
};

// Toggle sidebar state
const setSidebarCollapsed = (collapsed) => {
    state.sidebarCollapsed = collapsed;
    document.body.classList.toggle("sidebar-collapsed", collapsed);

    elements.sidebarToggle?.setAttribute(
        "aria-label",
        collapsed ? "Expand sidebar" : "Collapse sidebar"
    );

    send("ui.sidebar.toggle", { collapsed });
};

// Navigate from address bar
const navigateFromAddress = () => {
    const value = elements.address?.value.trim();
    if (value) {
        send("nav.go", { url: value });
    }
};

// Command palette commands
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

// Render command palette results
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

// Open command palette
const openPalette = () => {
    document.body.classList.add("palette-open");

    if (elements.paletteInput) {
        elements.paletteInput.value = "";
        state.paletteIndex = 0;
        renderPaletteResults("");
        elements.paletteInput.focus();
    }
};

// Close command palette
const closePalette = () => {
    document.body.classList.remove("palette-open");
    elements.paletteInput?.blur();
};

// Event handlers setup
const setupEventHandlers = () => {
    // Address bar navigation
    elements.address?.addEventListener("keydown", ({ key }) => {
        if (key === "Enter") navigateFromAddress();
    });

        elements.go?.addEventListener("click", navigateFromAddress);

        // Navigation buttons
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

        // Sidebar peek mode (Alt key)
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

        // Command palette events
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

        // Global keyboard shortcuts
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

            if (event.key === "Escape" && document.body.classList.contains("palette-open")) {
                closePalette();
            }
        });

        // Tab navigation with arrow keys
        elements.tabs?.addEventListener("keydown", (event) => {
            if (!state.flatTabs.length) return;

            if (event.key === "ArrowDown" || event.key === "ArrowUp") {
                event.preventDefault();

                const currentIndex = state.flatTabs.indexOf(state.activeTabId);
                const delta = event.key === "ArrowDown" ? 1 : -1;
                const nextIndex = currentIndex === -1
                ? 0
                : Math.min(Math.max(currentIndex + delta, 0), state.flatTabs.length - 1);

                const nextId = state.flatTabs[nextIndex];
                if (nextId != null) {
                    send("tab.select", { id: nextId });
                }
            }
        });
};

// Message receiver from native bridge
window.__owl_receive = (message) => {
    if (!message?.type) return;

    const messageHandlers = {
        "state.tabs": () => applyState(message.payload),
        "state.nav": () => applyNavState(message.payload),
    };

    messageHandlers[message.type]?.();
};

// Initialize on DOM ready
window.addEventListener("DOMContentLoaded", () => {
    setupEventHandlers();
    send("ui.ready");
});
