# OwL Browser Architecture (Target)

This document defines the target architecture for OwL Browser. It is written for implementation planning and review, not for marketing.

**Scope**
- Platform: Linux only, Wayland-first, Fedora 43 primary target.
- UI stack: GTK4 + libadwaita.
- Web engine: WebKitGTK (JavaScriptCore).
- Policy: Website JavaScript is not modified or rewritten. Rust controls execution through scheduling and resource governance only.

**High-Level Architecture Overview**
- Rust Core is the control plane. It owns tab lifecycle, navigation policy, scheduling, budgets, persistence, and security decisions.
- Web Engine Layer is WebKitGTK. It renders, networks, and executes JavaScriptCore without modification.
- Browser UI is HTML/CSS rendered in a dedicated WebView. It is a client of the Rust Core and never a peer.

**Process Model**
- UI Process: Rust + GTK4/libadwaita. Hosts the chrome WebView and owns the control plane.
- UI WebView Process: Dedicated WebKit WebView for chrome, with a restricted message bridge.
- Web Content Processes: One web process per tab for isolation and enforceable resource limits. Network/media subprocesses are managed by WebKitGTK.

**Layer Separation And Communication**
- Rust Core and the UI WebView exchange structured messages over a single authenticated bridge.
- Messages are schema-validated and versioned. The bridge exposes only explicit commands and state updates.
- The UI WebView never receives direct handles to web content processes.
- All navigation, tab switching, and policy decisions originate in Rust Core.

**JavaScript Execution Control (Scheduling Without Code Modification)**
- Rust observes page state using visibility, focus, and activity signals from WebKitGTK and the window system.
- Rust assigns a tab class: `active`, `background`, `hidden`.
- Rust enforces per-tab budgets using OS scheduling and WebKitGTK throttling knobs, without altering JavaScript.

**Control Mechanisms**
- CPU budgets: cgroups v2 to cap background web processes and prioritize the active tab.
- Priority classes: active tab gets higher CPU share and normal I/O priority.
- Visibility signaling: propagate tab state to WebKit so the engine applies standards-aligned throttling for timers and rAF cadence.
- Idle policies: hidden tabs can be paused or discarded under pressure with explicit reload on return.

**Why This Preserves Compatibility**
- JavaScript executes unmodified in JavaScriptCore.
- The browser only changes scheduling and resource allocation.
- Visibility-based throttling matches web platform expectations instead of rewriting code.

**Multi-Tab Behavior**
- Each tab has its own web process and resource budget.
- Background tabs are throttled by CPU and I/O limits.
- Memory growth is bounded by per-process limits and a discard policy under pressure.
- Discarding is explicit. The user returns to a reloaded page rather than a silently modified JS environment.

**Basic Rust Module Layout**
- `src/core/app.rs`
- `src/core/tabs.rs`
- `src/core/scheduler.rs`
- `src/core/resources.rs`
- `src/core/policy.rs`
- `src/core/engine/mod.rs`
- `src/core/engine/webkit.rs`
- `src/core/ipc.rs`
- `src/core/storage.rs`
- `src/ui/bridge.rs`
- `src/ui/shell.rs`
- `src/platform/linux/cgroups.rs`
- `src/platform/linux/wayland.rs`

**Why This Foundation Can Scale**
- The control plane is explicit and isolated from rendering and JS execution.
- Per-tab process isolation enables predictable CPU and memory enforcement.
- Policies evolve in Rust without changing web content or WebKit internals.
- The architecture supports future constraints like stricter sandboxing or new scheduling strategies without a rewrite.
