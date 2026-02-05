// Modern ES6+ rewrite with improved robustness and performance

// Cache DOM elements
const elements = {
  searchInput: document.getElementById("search"),
  searchButton: document.getElementById("search-btn"),
  focusToggle: document.getElementById("focus-mode"),
};

// URL validation using URL constructor
const isValidUrl = (string) => {
  try {
    new URL(string);
    return true;
  } catch {
    return string.startsWith("about:") || string.startsWith("owl://");
  }
};

// Search handler with optional chaining
const runSearch = () => {
  const query = elements.searchInput?.value.trim();

  if (!query) return;

  const destination = isValidUrl(query)
  ? query
  : `https://duckduckgo.com/?q=${encodeURIComponent(query)}`;

  window.location.href = destination;
};

// Event delegation for link clicks
const handleLinkClick = (target) => {
  const url = target.dataset.url;
  const session = target.dataset.session;

  if (url) {
    window.location.href = url;
    return true;
  }

  if (session) {
    window.location.href = `owl://session/${session}`;
    return true;
  }

  return false;
};

// Unified event handler for keyboard and click events
const handleInteraction = (event) => {
  if (event.type === "click" || event.key === "Enter") {
    event.preventDefault();
    handleLinkClick(event.currentTarget);
  }
};

// Set up event listeners with optional chaining
elements.searchInput?.addEventListener("keydown", (event) => {
  if (event.key === "Enter") runSearch();
});

elements.searchButton?.addEventListener("click", runSearch);

elements.focusToggle?.addEventListener("change", ({ target }) => {
  document.body.classList.toggle("focus-mode", target.checked);
});

// Event delegation for better performance
document.querySelectorAll("[data-url], [data-session]").forEach((element) => {
  element.addEventListener("click", handleInteraction);
  element.addEventListener("keydown", handleInteraction);
});
