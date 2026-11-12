/* ============================================================
   Общие функции интерфейса: иконки, каркас (меню + шапка),
   форматирование дат, всплывающие окна, уведомления-тосты.
   ============================================================ */
function pad2(number) { return (number < 10 ? '0' : '') + number; }
function isoDate(dateObject) {
  return dateObject.getFullYear() + '-' + pad2(dateObject.getMonth() + 1) + '-' + pad2(dateObject.getDate());
}

var ICONS = {
  grid: '<rect x="3.5" y="3.5" width="7" height="7" rx="2"/><rect x="13.5" y="3.5" width="7" height="7" rx="2"/><rect x="3.5" y="13.5" width="7" height="7" rx="2"/><rect x="13.5" y="13.5" width="7" height="7" rx="2"/>',
  chat: '<path d="M20 11.5a7.5 7.5 0 0 1-11 6.6L4 19.5l1.4-4.3A7.5 7.5 0 1 1 20 11.5z"/><path d="M9 11h6M9 14h3.5"/>',
  doc: '<path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/><path d="M14 3v5h5M9 13h6M9 17h6"/>',
  tasks: '<rect x="4" y="4" width="16" height="16" rx="3.5"/><path d="M8.5 12.2l2.5 2.5 4.5-5"/>',
  calendar: '<rect x="3.5" y="5" width="17" height="15.5" rx="3"/><path d="M8 3v4M16 3v4M3.5 10h17"/>',
  bell: '<path d="M6 16V11a6 6 0 0 1 12 0v5l1.5 2h-15z"/><path d="M10 20.5a2.2 2.2 0 0 0 4 0"/>',
  gear: '<circle cx="12" cy="12" r="3.5"/><path d="M12 3v2.5M12 18.5V21M3 12h2.5M18.5 12H21M5.6 5.6l1.8 1.8M16.6 16.6l1.8 1.8M5.6 18.4l1.8-1.8M16.6 7.4l1.8-1.8"/>',
  search: '<circle cx="11" cy="11" r="6.5"/><path d="M16 16l4 4"/>',
  plus: '<path d="M12 5v14M5 12h14"/>',
  left: '<path d="M19 12H5M11 6l-6 6 6 6"/>',
  right: '<path d="M5 12h14M13 6l6 6-6 6"/>',
  up: '<path d="M12 19V5M6 11l6-6 6 6"/>',
  down: '<path d="M12 5v14M6 13l6 6 6-6"/>',
  chevron: '<path d="M6 9l6 6 6-6"/>',
  x: '<path d="M6 6l12 12M18 6L6 18"/>',
  check: '<path d="M5 12.5l4.5 4.5L19 7.5"/>',
  play: '<path d="M8 5v14l11-7z"/>',
  pause: '<path d="M8 5v14M16 5v14" stroke-width="3"/>',
  more: '<circle cx="5" cy="12" r="1.2"/><circle cx="12" cy="12" r="1.2"/><circle cx="19" cy="12" r="1.2"/>',
  menu: '<path d="M4 7h16M4 12h16M4 17h16"/>',
  clip: '<path d="M20 11l-8.5 8.5a5 5 0 0 1-7-7L13 4a3.3 3.3 0 0 1 4.7 4.7L9.2 17.2a1.7 1.7 0 0 1-2.4-2.4L14 7.6"/>',
  refresh: '<path d="M20 11a8 8 0 0 0-14.5-3.5L4 9M4 4v5h5M4 13a8 8 0 0 0 14.5 3.5L20 15M20 20v-5h-5"/>',
  share: '<path d="M12 15V4M7 8.5l5-5 5 5M5 13v5a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2v-5"/>'
};

function icon(name, size) {
  var style = size ? ' style="width:' + size + 'px;height:' + size + 'px"' : '';
  return '<svg class="i" viewBox="0 0 24 24"' + style + ' aria-hidden="true">' + ICONS[name] + '</svg>';
}

function esc(value) {
  return String(value === undefined || value === null ? '' : value)
    .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
}
function $(selector, root) { return (root || document).querySelector(selector); }
function $all(selector, root) { return Array.prototype.slice.call((root || document).querySelectorAll(selector)); }
function getParam(name) { return new URLSearchParams(window.location.search).get(name); }

/* ---------- даты и слова ---------- */
var MONTHS_SHORT = ['янв', 'фев', 'мар', 'апр', 'мая', 'июн', 'июл', 'авг', 'сен', 'окт', 'ноя', 'дек'];
var MONTHS_FULL = ['января', 'февраля', 'марта', 'апреля', 'мая', 'июня', 'июля', 'августа', 'сентября', 'октября', 'ноября', 'декабря'];
var WEEKDAYS_SHORT = ['Вс', 'Пн', 'Вт', 'Ср', 'Чт', 'Пт', 'Сб'];

function plural(count, one, few, many) {
  var n = Math.abs(count) % 100;
  var last = n % 10;
  if (n > 10 && n < 20) { return many; }
  if (last > 1 && last < 5) { return few; }
  if (last === 1) { return one; }
  return many;
}
function countText(count, one, few, many) { return count + ' ' + plural(count, one, few, many); }
function sameDay(a, b) { return a.getFullYear() === b.getFullYear() && a.getMonth() === b.getMonth() && a.getDate() === b.getDate(); }
function timeText(date) { return pad2(date.getHours()) + ':' + pad2(date.getMinutes()); }

function dayWord(date) {
  var today = new Date();
  var yesterday = new Date();
  yesterday.setDate(today.getDate() - 1);
  var tomorrow = new Date();
  tomorrow.setDate(today.getDate() + 1);
  if (sameDay(date, today)) { return 'Сегодня'; }
  if (sameDay(date, yesterday)) { return 'Вчера'; }
  if (sameDay(date, tomorrow)) { return 'Завтра'; }
  return date.getDate() + ' ' + MONTHS_SHORT[date.getMonth()];
}
function formatDateTime(iso) { var d = new Date(iso); return dayWord(d) + ', ' + timeText(d); }
function formatDay(iso) { return dayWord(new Date(iso)); }
function formatTime(iso) { return timeText(new Date(iso)); }
function isDueSoon(iso) { var d = new Date(iso); return sameDay(d, new Date()); }
function toInputDate(iso) { return isoDate(new Date(iso)); }

var STATUS = {
  in_progress: { label: 'В работе', tone: 'purple' },
  review: { label: 'На проверке', tone: 'orange' },
  planned: { label: 'Запланировано', tone: '' },
  done: { label: 'Завершено', tone: 'green' }
};
function statusBadge(status, extraClass) {
  var info = STATUS[status] || STATUS.planned;
  return '<span class="badge ' + info.tone + ' ' + (extraClass || '') + '">' + info.label + '</span>';
}

/* ---------- уведомления-тосты ---------- */
function toast(message, type) {
  var host = $('.toast-host');
  if (!host) { host = document.createElement('div'); host.className = 'toast-host'; document.body.appendChild(host); }
  var item = document.createElement('div');
  item.className = 'toast ' + (type || '');
  item.setAttribute('role', 'status');
  item.innerHTML = (type === 'error' ? '' : icon('check', 16)) + '<span>' + esc(message) + '</span><button aria-label="Закрыть">' + icon('x', 14) + '</button>';
  host.appendChild(item);
  function remove() { if (item.parentNode) { item.parentNode.removeChild(item); } }
  item.querySelector('button').addEventListener('click', remove);
  setTimeout(remove, 4200);
}
function showError(error) { toast(error && error.message ? error.message : 'Что-то пошло не так', 'error'); }

/* ---------- модальные окна ---------- */
function openModal(config) {
  var backdrop = document.createElement('div');
  backdrop.className = 'modal-back';
  backdrop.innerHTML = '<div class="modal ' + (config.narrow ? 'narrow' : '') + '" role="dialog" aria-modal="true" aria-label="' + esc(config.title) + '">' +
    '<div class="modal-head"><h2>' + esc(config.title) + '</h2><button class="icon-btn" data-close aria-label="Закрыть">' + icon('x', 18) + '</button></div>' +
    '<div class="modal-body">' + config.html + '</div></div>';
  document.body.appendChild(backdrop);
  var previousFocus = document.activeElement;
  function close() {
    document.removeEventListener('keydown', onKey);
    if (backdrop.parentNode) { backdrop.parentNode.removeChild(backdrop); }
    if (previousFocus && previousFocus.focus) { previousFocus.focus(); }
  }
  function onKey(event) { if (event.key === 'Escape') { close(); } }
  document.addEventListener('keydown', onKey);
  backdrop.addEventListener('mousedown', function (event) { if (event.target === backdrop) { close(); } });
  var closers = $all('[data-close]', backdrop);
  for (var i = 0; i < closers.length; i++) { closers[i].addEventListener('click', close); }
  var firstInput = $('input,select,textarea,button.primary', backdrop.querySelector('.modal-body'));
  if (firstInput) { firstInput.focus(); }
  return { root: backdrop, close: close };
}

/* ---------- формы ---------- */
function setFieldError(fieldElement, message) {
  fieldElement.classList.add('error');
  var errorBox = $('.field-error', fieldElement);
  if (errorBox) { errorBox.textContent = message; }
}
function clearFieldError(fieldElement) { fieldElement.classList.remove('error'); }
function watchFieldErrors(root) {
  var inputs = $all('.field .input, .field .select, .field .textarea', root);
  for (var i = 0; i < inputs.length; i++) {
    inputs[i].addEventListener('input', function (event) { clearFieldError(event.target.closest('.field')); });
  }
}
function setBusy(button, busy, busyText) {
  if (busy) { button.dataset.text = button.innerHTML; button.classList.add('busy'); if (busyText) { button.textContent = busyText; } }
  else { button.classList.remove('busy'); if (button.dataset.text) { button.innerHTML = button.dataset.text; } }
}
function optionsHtml(items, selectedValue, valueKey, labelKey) {
  var html = '';
  for (var i = 0; i < items.length; i++) {
    var value = valueKey ? items[i][valueKey] : items[i];
    var label = labelKey ? items[i][labelKey] : items[i];
    html += '<option value="' + esc(value) + '"' + (value === selectedValue ? ' selected' : '') + '>' + esc(label) + '</option>';
  }
  return html;
}
function debounce(callback, delayMs) {
  var timer = null;
  return function () {
    var args = arguments;
    clearTimeout(timer);
    timer = setTimeout(function () { callback.apply(null, args); }, delayMs);
  };
}
function attachMenu(anchor, items, onPick) {
  closeMenus();
  var menu = document.createElement('div');
  menu.className = 'menu';
  var html = '';
  for (var i = 0; i < items.length; i++) { html += '<button data-value="' + esc(items[i].value) + '" class="' + (items[i].danger ? 'danger' : '') + '">' + esc(items[i].label) + '</button>'; }
  menu.innerHTML = html;
  var rect = anchor.getBoundingClientRect();
  menu.style.top = (window.scrollY + rect.bottom + 6) + 'px';
  menu.style.left = Math.max(8, window.scrollX + rect.right - 180) + 'px';
  document.body.appendChild(menu);
  menu.addEventListener('click', function (event) {
    var button = event.target.closest('button');
    if (button) { closeMenus(); onPick(button.dataset.value); }
  });
  setTimeout(function () { document.addEventListener('click', closeMenus, { once: true }); }, 0);
}
function closeMenus() { var menus = $all('body > .menu'); for (var i = 0; i < menus.length; i++) { menus[i].remove(); } }

function downloadText(fileName, content) {
  var blob = new Blob([content], { type: 'text/plain;charset=utf-8' });
  var link = document.createElement('a');
  link.href = URL.createObjectURL(blob);
  link.download = fileName;
  document.body.appendChild(link);
  link.click();
  link.remove();
}

/* ---------- каркас страницы ---------- */
var NAV_ITEMS = [
  { key: 'overview', href: 'overview.html', label: 'Обзор', icon: 'grid' },
  { key: 'assistant', href: 'assistant.html', label: 'AI-ассистент', icon: 'chat' },
  { key: 'meetings', href: 'meetings.html', label: 'Саммари встреч', icon: 'doc' },
  { key: 'tasks', href: 'tasks.html', label: 'Задачи', icon: 'tasks', badge: true },
  { key: 'schedule', href: 'schedule.html', label: 'График работы', icon: 'calendar' },
  { key: 'notifications', href: 'notifications.html', label: 'Уведомления', icon: 'bell', dot: true }
];

function renderShell() {
  var activeKey = document.body.dataset.nav;
  var title = document.body.dataset.title || '';
  var navHtml = '';
  for (var i = 0; i < NAV_ITEMS.length; i++) {
    var item = NAV_ITEMS[i];
    navHtml += '<a class="nav-item' + (item.key === activeKey ? ' active' : '') + '" href="' + item.href + '"' + (item.key === activeKey ? ' aria-current="page"' : '') + '>' +
      icon(item.icon) + '<span>' + item.label + '</span>' + (item.badge ? '<em class="nav-badge hidden" id="nav-tasks-count"></em>' : '') + (item.dot ? '<em class="nav-dot hidden" id="nav-unread-dot"></em>' : '') + '</a>';
  }
  $('#sidebar').innerHTML =
    '<a class="logo" href="overview.html"><b>фотопринт</b><small>AI Workspace</small></a>' +
    '<a class="btn-new" href="new-meeting.html">' + icon('plus', 18) + '<span>Новая встреча</span></a>' +
    '<nav class="nav" aria-label="Основная навигация">' + navHtml + '</nav>' +
    '<div class="side-group"><small>Рабочее пространство</small><span id="workspace-name">&nbsp;</span></div>' +
    '<div class="side-bottom"><a class="nav-item' + (activeKey === 'settings' ? ' active' : '') + '" href="settings.html">' + icon('gear') + '<span>Настройки</span></a>' +
    '<a class="user-card" href="settings.html"><b id="user-name">&nbsp;</b><small id="user-role">&nbsp;</small></a>' +
    '<a class="nav-item" href="login.html" id="logout"><span>Выйти</span></a></div>';

  $('#topbar').innerHTML =
    '<button class="burger" id="burger" aria-label="Меню">' + icon('menu') + '</button>' +
    '<div class="crumbs"><span>Рабочее пространство</span><span>/</span><b>' + esc(title) + '</b></div>' +
    '<div class="search rel" id="global-search">' + icon('search', 18) + '<input type="search" placeholder="Поиск по рабочему пространству" aria-label="Поиск"></div>' +
    '<a class="bell" href="notifications.html" aria-label="Уведомления">' + icon('bell') + '<i class="hidden" id="bell-dot"></i></a>' +
    '<a class="avatar" href="settings.html" id="avatar">&nbsp;</a>';

  var overlay = document.createElement('div');
  overlay.className = 'overlay-nav';
  document.body.appendChild(overlay);
  $('#burger').addEventListener('click', function () { document.body.classList.toggle('nav-open'); });
  overlay.addEventListener('click', function () { document.body.classList.remove('nav-open'); });

  setupGlobalSearch();
  fillShellData();
  $('#logout').addEventListener('click', async function (event) {
    event.preventDefault();
    try { await API.logout(); } catch (error) { /* всё равно уходим на страницу входа */ }
    window.location.href = CONFIG.LOGIN_URL;
  });
}

function fillShellData() {
  API.getMe().then(function (me) {
    $('#user-name').textContent = me.name;
    $('#user-role').textContent = me.role;
    $('#avatar').textContent = me.initials;
  }).catch(showError);
  API.getSettings().then(function (settings) { $('#workspace-name').textContent = settings.company + ' · команда'; }).catch(function () {});
  API.getTasks({ status: 'active', pageSize: 1 }).then(function (data) {
    var badge = $('#nav-tasks-count');
    if (data.counts.active > 0) { badge.textContent = data.counts.active; badge.classList.remove('hidden'); }
  }).catch(function () {});
  API.getNotifications('all').then(function (data) {
    if (data.counts.unread > 0) { $('#nav-unread-dot').classList.remove('hidden'); $('#bell-dot').classList.remove('hidden'); }
  }).catch(function () {});
}

function setupGlobalSearch() {
  var box = $('#global-search');
  var input = $('input', box);
  var run = debounce(function () {
    var text = input.value.trim();
    var old = $('.menu', box);
    if (old) { old.remove(); }
    if (text.length < 2) { return; }
    API.search(text).then(function (results) {
      var menu = document.createElement('div');
      menu.className = 'menu';
      menu.style.top = '46px'; menu.style.left = '0'; menu.style.right = '0';
      if (results.length === 0) { menu.innerHTML = '<button disabled>Ничего не нашлось</button>'; }
      for (var i = 0; i < results.length; i++) {
        var link = document.createElement('button');
        var target = results[i].type === 'meeting' ? 'meeting.html?id=' : 'task.html?id=';
        link.dataset.href = target + results[i].id;
        link.textContent = (results[i].type === 'meeting' ? 'Встреча · ' : 'Задача · ') + results[i].title;
        menu.appendChild(link);
      }
      menu.addEventListener('click', function (event) {
        var button = event.target.closest('button');
        if (button && button.dataset.href) { window.location.href = button.dataset.href; }
      });
      box.appendChild(menu);
    }).catch(showError);
  }, 300);
  input.addEventListener('input', run);
  document.addEventListener('click', function (event) {
    if (!box.contains(event.target)) { var menu = $('.menu', box); if (menu) { menu.remove(); } }
  });
}

document.addEventListener('DOMContentLoaded', function () {
  if ($('#sidebar')) { renderShell(); }
});

/* ---------- общие окна, которые нужны сразу на нескольких страницах ---------- */
function openTaskModal(employees, existingTask, onSaved) {
  var isEdit = !!existingTask;
  var task = existingTask || { title: '', assigneeId: employees[0] ? employees[0].id : '', due: daysFromTodayIso(1, 12, 0), priority: 'Обычный', description: '' };
  var dueValue = toInputDate(task.due);
  var timeValue = formatTime(task.due);
  var modal = openModal({
    title: isEdit ? 'Редактировать задачу' : 'Новая задача',
    html: '<div class="field"><label>Название *</label><input class="input" name="title" value="' + esc(task.title) + '" placeholder="Что нужно сделать"><span class="field-error"></span></div>' +
      '<div class="row"><div class="field"><label>Ответственный</label><select class="select" name="assignee">' + optionsHtml(employees, task.assigneeId, 'id', 'name') + '</select></div>' +
      '<div class="field"><label>Приоритет</label><select class="select" name="priority">' + optionsHtml(['Обычный', 'Высокий', 'Срочный'], task.priority) + '</select></div></div>' +
      '<div class="row"><div class="field"><label>Срок: дата *</label><input class="input" type="date" name="date" value="' + dueValue + '"><span class="field-error"></span></div>' +
      '<div class="field"><label>Время</label><input class="input" type="time" name="time" value="' + timeValue + '"></div></div>' +
      '<div class="field"><label>Описание</label><textarea class="textarea" name="description" style="min-height:80px">' + esc(task.description) + '</textarea></div>' +
      '<div class="modal-foot"><button class="btn secondary" data-close>Отмена</button><button class="btn primary" id="task-save">' + (isEdit ? 'Сохранить' : 'Создать задачу') + '</button></div>'
  });
  var root = modal.root;
  watchFieldErrors(root);
  $('#task-save', root).addEventListener('click', async function (event) {
    var button = event.currentTarget;
    var titleInput = $('[name=title]', root);
    var dateInput = $('[name=date]', root);
    var valid = true;
    if (!titleInput.value.trim()) { setFieldError(titleInput.closest('.field'), 'Введите название задачи'); valid = false; }
    if (!dateInput.value) { setFieldError(dateInput.closest('.field'), 'Укажите срок'); valid = false; }
    if (!valid) { return; }
    var payload = {
      title: titleInput.value.trim(), assigneeId: $('[name=assignee]', root).value, priority: $('[name=priority]', root).value,
      description: $('[name=description]', root).value.trim(), due: dateInput.value + 'T' + ($('[name=time]', root).value || '12:00')
    };
    setBusy(button, true, 'Сохраняем…');
    try {
      var saved = isEdit ? await API.updateTask(existingTask.id, { title: payload.title, assigneeId: payload.assigneeId, priority: payload.priority, description: payload.description, due: new Date(payload.due).toISOString() }) : await API.createTask(payload);
      modal.close();
      toast(isEdit ? 'Задача обновлена' : 'Задача создана');
      if (onSaved) { onSaved(saved); }
    } catch (error) { setBusy(button, false); showError(error); }
  });
}
function daysFromTodayIso(offset, hours, minutes) {
  if (typeof daysFromToday === 'function') { return daysFromToday(offset, hours, minutes); }
  var date = new Date(); date.setDate(date.getDate() + offset); date.setHours(hours, minutes || 0, 0, 0); return date.toISOString();
}

function openReminderModal(task, onSaved) {
  var beforeOptions = [{ v: '15m', l: 'За 15 минут до срока' }, { v: '1h', l: 'За 1 час до срока' }, { v: '1d', l: 'За 1 день до срока' }, { v: '0', l: 'В момент срока' }];
  var channels = ['Telegram и почта', 'Только Telegram', 'Только почта'];
  var modal = openModal({
    title: 'Напомнить о задаче', narrow: true,
    html: '<p class="muted" style="font-size:12px">' + esc(task.title) + '</p>' +
      '<div class="row"><div class="field"><label>Когда</label><select class="select" name="before">' + optionsHtml(beforeOptions, task.reminder ? task.reminder.before : '1h', 'v', 'l') + '</select></div>' +
      '<div class="field"><label>Канал</label><select class="select" name="channel">' + optionsHtml(channels, task.reminder ? task.reminder.channel : channels[0]) + '</select></div></div>' +
      '<p class="hint">Получатель: ответственный за задачу.<br>Тихие часы учитываются автоматически.</p>' +
      '<div class="modal-foot"><button class="btn primary" id="rem-save">Сохранить</button></div>'
  });
  $('#rem-save', modal.root).addEventListener('click', async function (event) {
    var button = event.currentTarget;
    setBusy(button, true, 'Сохраняем…');
    try {
      var saved = await API.setReminder(task.id, { before: $('[name=before]', modal.root).value, channel: $('[name=channel]', modal.root).value });
      modal.close(); toast('Изменения сохранены'); if (onSaved) { onSaved(saved); }
    } catch (error) { setBusy(button, false); showError(error); }
  });
}

/* простая проверка: если не демо-режим и нет сессии, бэк вернёт 401 и api.js отправит на login.html */

/* ---------- файлы встреч (используются на «Обзоре» и в «Новой встрече») ---------- */
var ALLOWED_EXTENSIONS = ['txt', 'docx', 'pdf', 'mp3', 'wav', 'm4a', 'ogg', 'oga', 'opus', 'webm', 'aac', 'flac', 'mp4'];
var MAX_FILE_MB = 100;

function validateMeetingFile(file) {
  var extension = file.name.split('.').pop().toLowerCase();
  if (ALLOWED_EXTENSIONS.indexOf(extension) < 0) { return 'Формат .' + extension + ' не поддерживается. Подойдут: TXT, DOCX, PDF и аудио (MP3, WAV, M4A, OGG, голосовые).'; }
  if (file.size > MAX_FILE_MB * 1024 * 1024) { return 'Файл больше ' + MAX_FILE_MB + ' МБ.'; }
  return '';
}
function fileSizeText(bytes) {
  if (bytes > 1024 * 1024) { return (bytes / 1024 / 1024).toFixed(1).replace('.', ',') + ' МБ'; }
  return Math.max(1, Math.round(bytes / 1024)) + ' КБ';
}
function meetingMetaText(meeting) {
  var date = new Date(meeting.date);
  var parts = [dayWord(date) + ', ' + timeText(date)];
  if (meeting.durationMin) { parts.push(meeting.durationMin + ' мин'); }
  parts.push(countText(meeting.participantIds.length, 'участник', 'участника', 'участников'));
  return parts.join(' · ');
}
function meetingStatusBadge(meeting) {
  if (meeting.status === 'processing') { return '<span class="badge purple">Обработка</span>'; }
  if (meeting.status === 'error') { return '<span class="badge red">Ошибка</span>'; }
  return '<span class="badge green">Готово</span>';
}
/* Блок «Обработка записи» (состояние 03) и «Ошибка загрузки» (состояние 04) */
function processingHtml(progress) {
  return '<span class="badge purple">Обработка · ' + progress + '%</span><h2>Собираю саммари встречи</h2>' +
    '<p>Запись распознана. Выделяю решения и задачи.<br>Уведомлю, когда всё будет готово.</p>' +
    '<div class="progress" style="margin:26px 0 14px"><i style="width:' + progress + '%"></i></div>' +
    '<div style="display:flex;justify-content:space-between;align-items:center"><span class="hint">Можно продолжить работу</span><a class="link" href="meetings.html">Перейти к встречам ' + icon('right') + '</a></div>';
}
function errorHtml(message, title, audioUrl) {
  var player = audioUrl ? '<audio controls src="' + esc(audioUrl) + '" style="width:100%;margin-top:16px"></audio>' : '';
  return '<span class="badge red">Не удалось обработать</span><h2>' + esc(title || 'Соединение прервалось') + '</h2>' + player +
    '<p>' + esc(message || 'Не удалось завершить загрузку записи.') + '<br>Название встречи и участники сохранены.</p>' +
    '<div style="display:flex;gap:12px;margin-top:24px"><button class="btn primary lg" id="pick-other">Выбрать другой файл</button><button class="btn lg" id="retry-upload">Повторить загрузку</button></div>';
}
/* опрос статуса: вызывает onProgress(p) каждые 1.2 с и onReady() когда саммари собрано */
function watchMeeting(meetingId, onProgress, onReady, onFail) {
  var timer = setInterval(async function () {
    try {
      var status = await API.getMeetingStatus(meetingId);
      onProgress(status.progress);
      if (status.status === 'ready') { clearInterval(timer); onReady(); }
      if (status.status === 'error') { clearInterval(timer); onFail(); }
    } catch (error) { clearInterval(timer); onFail(error); }
  }, 1200);
  return timer;
}

/* Shared confirmation for irreversible user-requested removals. */
function confirmRemoval(title, description, remove) {
  var modal = openModal({ title: title, narrow: true,
    html: '<p style="white-space:pre-wrap;line-height:1.5">' + esc(description) + '</p><div class="modal-foot"><button class="btn" data-close>Отмена</button><button class="btn primary" data-confirm-removal>Удалить</button></div>' });
  var button = $('[data-confirm-removal]', modal.root);
  button.addEventListener('click', async function () {
    if (button.disabled) { return; }
    button.disabled = true; button.textContent = 'Удаляю…';
    try { await remove(); modal.close(); }
    catch (error) { showError(error); button.disabled = false; button.textContent = 'Удалить'; }
  });
}
var deletionPermission = null;
function configureDeletionControls() {
  if (!deletionPermission) { deletionPermission = API.getMe().then(function (me) { return me.appRole === 'admin' || me.appRole === 'manager'; }).catch(function () { return false; }); }
  return deletionPermission.then(function (allowed) { $all('[data-manager-delete]').forEach(function (button) { button.classList.toggle('hidden', !allowed); }); return allowed; });
}
