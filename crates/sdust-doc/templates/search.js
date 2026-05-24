// sdust-doc client-side search: fetch search-index.json, filter as user types.
(async function () {
  const q = document.getElementById("q");
  const out = document.getElementById("results");
  if (!q || !out) return;
  let idx = [];
  try {
    const r = await fetch("search-index.json");
    idx = await r.json();
  } catch (e) {
    return;
  }
  function render(list) {
    out.innerHTML = "";
    for (const it of list.slice(0, 50)) {
      const li = document.createElement("li");
      const a = document.createElement("a");
      a.href = it.url;
      a.textContent = it.name;
      const tag = document.createElement("span");
      tag.className = "kind";
      tag.textContent = it.kind;
      const syn = document.createElement("span");
      syn.className = "syn";
      syn.textContent = it.synopsis ? " — " + it.synopsis : "";
      li.appendChild(tag);
      li.appendChild(document.createTextNode(" "));
      li.appendChild(a);
      li.appendChild(syn);
      out.appendChild(li);
    }
  }
  function score(item, needle) {
    const n = needle.toLowerCase();
    const name = item.name.toLowerCase();
    if (name === n) return 1000;
    if (name.startsWith(n)) return 500;
    if (name.includes(n)) return 100;
    if ((item.synopsis || "").toLowerCase().includes(n)) return 10;
    return 0;
  }
  q.addEventListener("input", () => {
    const needle = q.value.trim();
    if (!needle) {
      out.innerHTML = "";
      return;
    }
    const scored = idx
      .map((it) => ({ it, s: score(it, needle) }))
      .filter((x) => x.s > 0)
      .sort((a, b) => b.s - a.s)
      .map((x) => x.it);
    render(scored);
  });
})();
