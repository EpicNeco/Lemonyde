// Lemonyde site — terminal-log hero animation + mobile nav toggle.
// No frameworks, no tracking, no network calls.

(function () {
  "use strict";

  // ---- Header show/hide toggle (hidden by default on load) ----
  var headerToggle = document.getElementById("header-toggle");
  var header = document.getElementById("site-header");
  var toggleIcon = headerToggle ? headerToggle.querySelector(".header-toggle-icon") : null;

  function setHeaderHidden(hidden) {
    document.body.classList.toggle("header-hidden", hidden);
    header.classList.toggle("is-hidden", hidden);
    headerToggle.setAttribute("aria-expanded", String(!hidden));
    headerToggle.setAttribute("aria-label", hidden ? "Show navigation" : "Hide navigation");
    if (toggleIcon) toggleIcon.textContent = hidden ? "⌄" : "⌃";
  }

  if (headerToggle && header) {
    setHeaderHidden(true); // hidden by default
    headerToggle.addEventListener("click", function () {
      var currentlyHidden = header.classList.contains("is-hidden");
      setHeaderHidden(!currentlyHidden);
    });
  }

  // ---- Mobile nav toggle ----
  var toggle = document.querySelector(".nav-toggle");
  var mobileNav = document.getElementById("mobile-nav");
  if (toggle && mobileNav) {
    toggle.addEventListener("click", function () {
      var open = toggle.getAttribute("aria-expanded") === "true";
      toggle.setAttribute("aria-expanded", String(!open));
      mobileNav.hidden = open;
    });
    mobileNav.querySelectorAll("a").forEach(function (link) {
      link.addEventListener("click", function () {
        toggle.setAttribute("aria-expanded", "false");
        mobileNav.hidden = true;
      });
    });
  }

  // ---- Terminal hero animation ----
  // Mirrors the app's own real Activity Log wording (see src/main.rs),
  // not an invented demo — this is what Lemonyde actually prints.
  var lines = [
    { text: "$ ./install.sh", pause: 350 },
    { text: "Installing Lemonyde to ~/.local/share/lemonyde", pause: 300 },
    { text: "$ lemonyde", pause: 450 },
    { text: "Sober is not installed — install it now via Flathub? [y/N] y", pause: 300 },
    { text: "$ flatpak install --user -y flathub org.vinegarhq.Sober", pause: 250 },
    { text: "Sober installed", pause: 500 },
    { text: "$ Launch Instances → Slot 1", pause: 300 },
    { text: "Slot 1 launched — sign in once, it's remembered next time", pause: 900 },
  ];

  var target = document.getElementById("term-log");
  if (!target) return;

  var reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  if (reduceMotion) {
    target.textContent = lines.map(function (l) { return l.text; }).join("\n");
    return;
  }

  var lineIndex = 0;
  var charIndex = 0;
  var buffer = "";

  function typeStep() {
    if (lineIndex >= lines.length) {
      // Hold for a beat, then restart the loop.
      setTimeout(function () {
        buffer = "";
        lineIndex = 0;
        charIndex = 0;
        target.textContent = "";
        setTimeout(typeStep, 500);
      }, 2200);
      return;
    }

    var current = lines[lineIndex];
    if (charIndex <= current.text.length) {
      target.textContent = buffer + current.text.slice(0, charIndex);
      charIndex++;
      setTimeout(typeStep, 14 + Math.random() * 18);
    } else {
      buffer += current.text + "\n";
      target.textContent = buffer;
      lineIndex++;
      charIndex = 0;
      setTimeout(typeStep, current.pause);
    }
  }

  typeStep();
})();
