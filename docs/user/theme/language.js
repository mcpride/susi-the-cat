// Language switcher of the Susi help: links to the same page in the other languages.
// Every language is a book of its own, built side by side (help/en/, help/de/), so the page of
// another language is found by replacing the book root, online (https://) and offline (file://).
(function () {
    "use strict";
    const languages = [
        { code: "en", name: "English" },
        { code: "de", name: "Deutsch" },
    ];
    const current = document.documentElement.lang || "en";
    const bar = document.querySelector(".right-buttons");
    if (!bar || typeof path_to_root === "undefined") {
        return;
    }
    const root = new URL(path_to_root || "./", window.location.href).href;
    const page = window.location.href.startsWith(root) ? window.location.href.slice(root.length) : "";
    for (const lang of languages.slice().reverse()) {
        if (lang.code === current) {
            continue;
        }
        const link = document.createElement("a");
        link.className = "susi-language";
        link.href = new URL("../" + lang.code + "/" + page, root).href;
        link.hreflang = lang.code;
        link.lang = lang.code;
        link.title = lang.name;
        link.setAttribute("aria-label", lang.name);
        link.textContent = lang.code.toUpperCase();
        bar.prepend(link);
    }
})();
