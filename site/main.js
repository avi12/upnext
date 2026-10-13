// The page works without this file: it only types the example requests in
// "How it works" and adds a copy button to each command.
(function () {
  "use strict";

  var still = window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  // Type each example request, wait, erase it, then type the next.
  var typed = document.querySelector(".typed[data-requests]");
  if (typed && !still) {
    var requests = typed.getAttribute("data-requests").split("|");
    var which = 0;
    var shown = requests[0].length;
    var erasing = false;
    var step = function () {
      var text = requests[which];
      if (!erasing && shown === text.length) {
        erasing = true;
        return setTimeout(step, 2600);
      }
      if (erasing && shown === 0) {
        erasing = false;
        which = (which + 1) % requests.length;
        return setTimeout(step, 350);
      }
      shown += erasing ? -1 : 1;
      typed.textContent = requests[which].slice(0, shown);
      setTimeout(step, erasing ? 18 : 42 + Math.random() * 40);
    };
    setTimeout(step, 1200);
  }

  // A copy button on each command; if the browser refuses the clipboard, the
  // command is selected so Ctrl+C or a long press copies it.
  Array.prototype.forEach.call(document.querySelectorAll("pre > code"), function (code) {
    var button = document.createElement("button");
    button.type = "button";
    button.className = "copy-button";
    button.textContent = "Copy";
    button.setAttribute("aria-label", "Copy the command");
    var done = function (label) {
      button.textContent = label;
      setTimeout(function () {
        button.textContent = "Copy";
      }, 1600);
    };
    var select = function () {
      var range = document.createRange();
      range.selectNodeContents(code);
      var selection = window.getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
      done("Selected");
    };
    button.addEventListener("click", function () {
      if (navigator.clipboard && navigator.clipboard.writeText) {
        navigator.clipboard.writeText(code.textContent).then(function () {
          done("Copied");
        }, select);
      } else {
        select();
      }
    });
    code.parentNode.appendChild(button);
  });
})();
