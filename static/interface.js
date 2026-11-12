/* ---------- кастомные выпадающие списки ---------- */
function customSelectName(select) {
  if (select.getAttribute('aria-label')) { return select.getAttribute('aria-label'); }
  if (select.labels && select.labels.length) { return select.labels[0].textContent.trim(); }
  var field = select.closest('.field');
  var label = field && field.querySelector('label');
  if (label) { return label.textContent.trim(); }
  return select.name || 'Выбор';
}

function closeCustomSelects(except) {
  var open = $all('.custom-select.is-open');
  for (var i = 0; i < open.length; i++) {
    if (open[i] === except) { continue; }
    open[i].classList.remove('is-open');
    var trigger = $('.custom-select-trigger', open[i]);
    var menu = open[i]._customSelectMenu;
    if (trigger) { trigger.setAttribute('aria-expanded', 'false'); }
    if (menu) { menu.hidden = true; }
  }
}

function positionCustomSelectMenu(wrapper) {
  if (!wrapper || !wrapper.classList.contains('is-open')) { return; }
  var trigger = $('.custom-select-trigger', wrapper);
  var menu = wrapper._customSelectMenu;
  if (!trigger || !menu) { return; }
  var rect = trigger.getBoundingClientRect();
  var gap = 6;
  var viewportPadding = 10;
  var below = window.innerHeight - rect.bottom - viewportPadding - gap;
  var above = rect.top - viewportPadding - gap;
  var openAbove = below < 150 && above > below;
  var available = Math.max(100, openAbove ? above : below);
  menu.style.position = 'fixed';
  menu.style.left = Math.max(8, rect.left) + 'px';
  menu.style.width = Math.min(rect.width, window.innerWidth - 16) + 'px';
  menu.style.maxHeight = Math.min(280, available) + 'px';
  menu.style.top = openAbove
    ? Math.max(viewportPadding, rect.top - Math.min(menu.scrollHeight, available) - gap) + 'px'
    : (rect.bottom + gap) + 'px';
}

function refreshCustomSelect(select) {
  if (!select || !select._customSelect) { return; }
  var parts = select._customSelect;
  var trigger = parts.trigger;
  var menu = parts.menu;
  var selected = select.options[select.selectedIndex];
  var value = $('.custom-select-value', trigger);
  value.textContent = selected ? selected.textContent : 'Выберите значение';
  trigger.setAttribute('aria-label', customSelectName(select) + ': ' + (selected ? selected.textContent : 'не выбрано'));
  trigger.disabled = !!select.disabled;
  menu.innerHTML = '';
  for (var i = 0; i < select.options.length; i++) {
    var option = select.options[i];
    var button = document.createElement('button');
    button.type = 'button';
    button.className = 'custom-select-option' + (option.selected ? ' is-selected' : '');
    button.setAttribute('role', 'option');
    button.setAttribute('aria-selected', option.selected ? 'true' : 'false');
    button.dataset.optionIndex = String(i);
    button.textContent = option.textContent;
    button.disabled = option.disabled;
    menu.appendChild(button);
  }
}

function openCustomSelect(wrapper, focusSelected) {
  var select = $('select', wrapper);
  if (!select) { return; }
  closeCustomSelects(wrapper);
  refreshCustomSelect(select);
  wrapper.classList.add('is-open');
  var trigger = $('.custom-select-trigger', wrapper);
  var menu = wrapper._customSelectMenu;
  trigger.setAttribute('aria-expanded', 'true');
  menu.hidden = false;
  menu.style.display = '';
  positionCustomSelectMenu(wrapper);
  if (focusSelected) {
    var option = $('.custom-select-option.is-selected:not(:disabled)', menu) || $('.custom-select-option:not(:disabled)', menu);
    if (option) { option.focus(); }
  }
}

function enhanceCustomSelect(select) {
  if (!select || select.tagName !== 'SELECT' || select.multiple || select.size > 1 || select.dataset.customSelectReady === 'true') { return; }
  var wrapper = document.createElement('div');
  wrapper.className = 'custom-select';
  wrapper.style.width = select.style.width || '100%';
  if (select.style.flex) { wrapper.style.flex = select.style.flex; }
  if (select.style.minWidth) { wrapper.style.minWidth = select.style.minWidth; }
  if (select.style.maxWidth) { wrapper.style.maxWidth = select.style.maxWidth; }
  wrapper.style.marginLeft = select.style.marginLeft;
  wrapper.style.marginRight = select.style.marginRight;

  var trigger = document.createElement('button');
  trigger.type = 'button';
  trigger.className = 'custom-select-trigger';
  trigger.setAttribute('aria-haspopup', 'listbox');
  trigger.setAttribute('aria-expanded', 'false');
  trigger.innerHTML = '<span class="custom-select-value"></span><svg class="custom-select-chevron" viewBox="0 0 24 24" aria-hidden="true"><path d="m6 9 6 6 6-6"/></svg>';

  var menu = document.createElement('div');
  menu.className = 'custom-select-menu';
  menu.setAttribute('role', 'listbox');
  menu.hidden = true;

  var parent = select.parentNode;
  parent.insertBefore(wrapper, select);
  wrapper.appendChild(select);
  wrapper.appendChild(trigger);
  /* Меню выносится в body: так его больше не обрезают карточки,
     колонки и другие контейнеры с overflow/собственными слоями. */
  document.body.appendChild(menu);
  wrapper._customSelectMenu = menu;
  menu._select = select;

  select.dataset.customSelectReady = 'true';
  select.classList.add('custom-select-native');
  select.setAttribute('aria-hidden', 'true');
  select.tabIndex = -1;
  select._customSelect = { wrapper: wrapper, trigger: trigger, menu: menu };

  trigger.addEventListener('click', function () {
    if (wrapper.classList.contains('is-open')) { closeCustomSelects(); }
    else { openCustomSelect(wrapper, false); }
  });
  menu.addEventListener('click', function (event) {
    var optionButton = event.target.closest('.custom-select-option');
    if (!optionButton || optionButton.disabled) { return; }
    event.preventDefault();
    event.stopPropagation();

    select.selectedIndex = Number(optionButton.dataset.optionIndex);
    refreshCustomSelect(select);

    /* Закрываем именно это меню напрямую: оно находится в body,
       отдельно от обёртки, поэтому не полагаемся на поиск открытых меню. */
    wrapper.classList.remove('is-open');
    trigger.setAttribute('aria-expanded', 'false');
    menu.hidden = true;
    menu.style.display = 'none';

    select.dispatchEvent(new Event('change', { bubbles: true }));
    trigger.focus();
  });
  select.addEventListener('change', function () { refreshCustomSelect(select); });
  select.addEventListener('invalid', function (event) {
    event.preventDefault();
    trigger.focus();
    openCustomSelect(wrapper, false);
  });
  trigger.addEventListener('keydown', function (event) {
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      openCustomSelect(wrapper, true);
    }
  });
  menu.addEventListener('keydown', function (event) {
    var options = $all('.custom-select-option:not(:disabled)', menu);
    var current = options.indexOf(document.activeElement);
    var next = current;
    if (event.key === 'ArrowDown') { next = Math.min(options.length - 1, current + 1); }
    else if (event.key === 'ArrowUp') { next = Math.max(0, current - 1); }
    else if (event.key === 'Home') { next = 0; }
    else if (event.key === 'End') { next = options.length - 1; }
    else if (event.key === 'Escape') { event.preventDefault(); closeCustomSelects(); trigger.focus(); return; }
    else { return; }
    event.preventDefault();
    if (options[next]) { options[next].focus(); }
  });

  if (select.labels) {
    for (var i = 0; i < select.labels.length; i++) {
      select.labels[i].addEventListener('click', function (event) {
        event.preventDefault();
        trigger.focus();
        openCustomSelect(wrapper, false);
      });
    }
  }
  refreshCustomSelect(select);
}

function enhanceCustomSelects(root) {
  if (!root) { return; }
  if (root.matches && root.matches('select')) { enhanceCustomSelect(root); }
  var selects = root.querySelectorAll ? root.querySelectorAll('select') : [];
  for (var i = 0; i < selects.length; i++) { enhanceCustomSelect(selects[i]); }
}

function syncCustomSelects(root) {
  var selects = (root || document).querySelectorAll('select[data-custom-select-ready="true"]');
  for (var i = 0; i < selects.length; i++) { refreshCustomSelect(selects[i]); }
}

function setupCustomSelects() {
  enhanceCustomSelects(document);
  window.addEventListener('resize', function () {
    var open = $('.custom-select.is-open');
    if (open) { positionCustomSelectMenu(open); }
  });
  window.addEventListener('scroll', function () {
    var open = $('.custom-select.is-open');
    if (open) { positionCustomSelectMenu(open); }
  }, true);
  document.addEventListener('click', function (event) {
    var open = $('.custom-select.is-open');
    var menu = open && open._customSelectMenu;
    if (open && !open.contains(event.target) && !(menu && menu.contains(event.target))) {
      closeCustomSelects();
    }
  });
  // Consume Escape before a modal's document handler can dismiss unsaved fields.
  document.addEventListener('keydown', function (event) {
    var open = $('.custom-select.is-open');
    if (event.key === 'Escape' && open) {
      event.preventDefault();
      event.stopImmediatePropagation();
      var trigger = $('.custom-select-trigger', open);
      closeCustomSelects();
      if (trigger) { trigger.focus(); }
    }
  }, true);
  if (window.MutationObserver && document.body) {
    var observer = new MutationObserver(function (records) {
      var removed = false;
      for (var i = 0; i < records.length; i++) {
        var target = records[i].target;
        var select = target && (target.tagName === 'SELECT' ? target : target.closest && target.closest('select'));
        if (select) { refreshCustomSelect(select); }
        if (records[i].removedNodes.length) { removed = true; }
        for (var j = 0; j < records[i].addedNodes.length; j++) {
          var node = records[i].addedNodes[j];
          if (node.nodeType === 1) { enhanceCustomSelects(node); }
        }
      }
      if (removed) {
        $all('.custom-select-menu').forEach(function (menu) {
          if (!menu._select || !menu._select.isConnected) { menu.remove(); }
        });
      }
    });
    observer.observe(document.body, { childList: true, subtree: true, attributes: true, attributeFilter: ['disabled', 'label', 'selected'] });
  }
}

function injectInterfaceEffects() {
  if ($('#interface-effects-style')) { return; }
  var style = document.createElement('style');
  style.id = 'interface-effects-style';
  style.textContent = `
    /* Мягкое свечение под курсором на карточках */
    .card { position:relative; }
    .card::after { content:""; position:absolute; inset:0; border-radius:inherit; z-index:0; pointer-events:none; opacity:0; transition:opacity .22s ease; background:radial-gradient(230px circle at var(--glow-x,50%) var(--glow-y,50%), color-mix(in srgb, var(--accent) 13%, transparent), transparent 72%); }
    .card.cursor-glow-active::after { opacity:1; }
    /* Поиск мягко расширяется при фокусе */
    #global-search { position:relative; transition:width .24s ease, background-color .2s ease, box-shadow .2s ease; }
    #global-search.is-focused { width: min(340px, 38vw); background:var(--surface); box-shadow:0 0 0 2px color-mix(in srgb, var(--accent) 28%, transparent), 0 8px 24px rgba(35,25,75,.08); }
    #global-search input { transition:font-size .2s ease; }
    #global-search.is-focused input { font-size:12px; }
    #global-search .search-results { z-index:80; overflow:hidden; transform-origin:top center; animation:searchDropIn .2s cubic-bezier(.2,.8,.2,1) both; box-shadow:0 14px 34px rgba(20,18,35,.14); border:1px solid var(--border); }
    #global-search .search-results button { animation:searchItemIn .22s ease both; }
    #global-search .search-results button:nth-child(2) { animation-delay:25ms; }
    #global-search .search-results button:nth-child(3) { animation-delay:50ms; }
    #global-search .search-results button:nth-child(4) { animation-delay:75ms; }
    /* Аккуратная круглая рамка аватара */
    #avatar { width:38px; height:38px; flex:0 0 38px; display:inline-flex; align-items:center; justify-content:center; border-radius:50%; border:2px solid var(--accent); background:var(--accent-soft); box-shadow:0 0 0 3px color-mix(in srgb, var(--accent) 13%, transparent); transition:transform .2s ease, box-shadow .2s ease; overflow:hidden; }
    #avatar:hover { transform:translateY(-1px); box-shadow:0 0 0 4px color-mix(in srgb, var(--accent) 20%, transparent), 0 5px 14px color-mix(in srgb, var(--accent) 18%, transparent); }
    @keyframes searchDropIn { from { opacity:0; transform:translateY(-7px) scaleY(.97); } to { opacity:1; transform:translateY(0) scaleY(1); } }
    @keyframes searchItemIn { from { opacity:0; transform:translateX(-5px); } to { opacity:1; transform:translateX(0); } }
    @media (max-width:700px) { #global-search.is-focused { width:min(264px, 45vw); } }
    @media (prefers-reduced-motion:reduce) { #global-search, #avatar, #global-search .search-results, #global-search .search-results button { animation:none!important; transition:none!important; } }
  `;
  document.head.appendChild(style);
  document.addEventListener('pointermove', function (event) {
    var card = event.target.closest && event.target.closest('.card');
    var active = $('.card.cursor-glow-active');
    if (active && active !== card) { active.classList.remove('cursor-glow-active'); }
    if (!card) { return; }
    var rect = card.getBoundingClientRect();
    card.style.setProperty('--glow-x', (event.clientX - rect.left) + 'px');
    card.style.setProperty('--glow-y', (event.clientY - rect.top) + 'px');
    card.classList.add('cursor-glow-active');
  }, { passive:true });
  document.addEventListener('pointerout', function (event) {
    if (event.target === document.documentElement) {
      var active = $('.card.cursor-glow-active');
      if (active) { active.classList.remove('cursor-glow-active'); }
    }
  });
}

/* ============================================================
   Индикатор загрузки данных (.loader).
   В HTML достаточно добавить блоку атрибут data-loader
   (data-loader="sm" — уменьшенный): пока страница не заменит
   содержимое блока полученными данными, в нём крутится лоадер.
   Из JS: loaderHtml() / loaderHtml(true).
   ============================================================ */
function loaderHtml(small) {
  var bars = '';
  for (var i = 1; i <= 12; i++) { bars += '<div class="bar' + i + '"></div>'; }
  return '<div class="loader-box"><div class="loader' + (small ? ' sm' : '') + '" role="status" aria-label="Загрузка">' + bars + '</div></div>';
}
(function () {
  var targets = document.querySelectorAll('[data-loader]');
  for (var i = 0; i < targets.length; i++) {
    targets[i].innerHTML = loaderHtml(targets[i].getAttribute('data-loader') === 'sm');
  }
})();

/* ============================================================
   Тёмная тема: переключатель в левом нижнем углу сайдбара.
   Выбор хранится в localStorage (ключ fp_theme); атрибут
   data-theme на <html> выставляется ещё в <head> страниц,
   чтобы не было вспышки светлой темы при загрузке.
   ============================================================ */
var THEME_SWITCH_HTML =
  '<div class="theme-switch">' +
  '<input type="checkbox" id="theme-checkbox" />' +
  '<label for="theme-checkbox" title="Тёмная тема" aria-label="Переключить тёмную тему">' +
  '<div></div>' +
  '<span><svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="currentColor" class="w-6 h-6">' +
  '<path fill-rule="evenodd" d="M9.528 1.718a.75.75 0 01.162.819A8.97 8.97 0 009 6a9 9 0 009 9 8.97 8.97 0 003.463-.69.75.75 0 01.981.98 10.503 10.503 0 01-9.694 6.46c-5.799 0-10.5-4.701-10.5-10.5 0-4.368 2.667-8.112 6.46-9.694a.75.75 0 01.818.162z" clip-rule="evenodd"></path>' +
  '</svg></span>' +
  '<span><svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="currentColor" class="w-6 h-6">' +
  '<path d="M12 2.25a.75.75 0 01.75.75v2.25a.75.75 0 01-1.5 0V3a.75.75 0 01.75-.75zM7.5 12a4.5 4.5 0 119 0 4.5 4.5 0 01-9 0zM18.894 6.166a.75.75 0 00-1.06-1.06l-1.591 1.59a.75.75 0 101.06 1.061l1.591-1.59zM21.75 12a.75.75 0 01-.75.75h-2.25a.75.75 0 010-1.5H21a.75.75 0 01.75.75zM17.834 18.894a.75.75 0 001.06-1.06l-1.59-1.591a.75.75 0 10-1.061 1.06l1.59 1.591zM12 18a.75.75 0 01.75.75V21a.75.75 0 01-1.5 0v-2.25A.75.75 0 0112 18zM7.758 17.303a.75.75 0 00-1.061-1.06l-1.591 1.59a.75.75 0 001.06 1.061l1.591-1.59zM6 12a.75.75 0 01-.75.75H3a.75.75 0 010-1.5h2.25A.75.75 0 016 12zM6.697 7.757a.75.75 0 001.06-1.06l-1.59-1.591a.75.75 0 00-1.061 1.06l1.59 1.591z"></path>' +
  '</svg></span>' +
  '</label></div>';

function currentTheme() { return document.documentElement.getAttribute('data-theme') === 'dark' ? 'dark' : 'light'; }
function applyTheme(theme) {
  if (theme === 'dark') { document.documentElement.setAttribute('data-theme', 'dark'); }
  else { document.documentElement.removeAttribute('data-theme'); }
  var box = document.getElementById('theme-checkbox');
  if (box) { box.checked = theme === 'dark'; }
}
function setupThemeSwitch() {
  var box = document.getElementById('theme-checkbox');
  if (!box) { return; }
  box.checked = currentTheme() === 'dark';
  box.addEventListener('change', function () {
    var theme = box.checked ? 'dark' : 'light';
    applyTheme(theme);
    try { localStorage.setItem('fp_theme', theme); } catch (e) { /* хранилище недоступно — тема сменится до перезагрузки */ }
  });
  // тема синхронизируется между открытыми вкладками
  window.addEventListener('storage', function (event) { if (event.key === 'fp_theme') { applyTheme(event.newValue === 'dark' ? 'dark' : 'light'); } });
}

document.addEventListener('DOMContentLoaded', function () { injectInterfaceEffects(); setupCustomSelects(); });
