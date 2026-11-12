/* The Rust server owns dialogs, action validation and confirmation. */
var dialogs = [];
var current = null;
var sending = false;
var pendingDialog = null;
var dialogLoad = 0;
var resolving = Object.create(null);
var SUGGESTIONS = ['Создай задачу', 'Составь график на следующую неделю', 'Какие задачи требуют внимания?', 'Открой встречи'];

function syncCompose() {
  $('#send').disabled = sending || !current || !$('#input').value.trim();
  $('#input').disabled = !current;
  $('#send').setAttribute('aria-busy', sending ? 'true' : 'false');
}
function renderDialogs() {
  var html = '';
  dialogs.forEach(function (d) {
    html += '<button class="dialog-item ' + (current && d.id === current.id ? 'active' : '') + '" data-id="' + esc(d.id) + '"><b>' + esc(d.title) + '</b><small>' + esc(formatDateTime(d.updatedAt)) + '</small></button>';
  });
  $('#dialogs').innerHTML = html;
}
async function refreshDialogs() {
  dialogs = await API.getDialogs();
  renderDialogs();
}
async function loadDialogs() {
  try {
    await refreshDialogs();
    if (!dialogs.length) { dialogs.push(await API.createDialog()); }
    await openDialog(dialogs[0].id);
    $('#assistant-status').textContent = 'Диалоги подключены';
  } catch (error) {
    $('#assistant-status').textContent = 'Нет подключения';
    $('#chat').innerHTML = '<p class="muted">Не удалось загрузить диалоги.</p><button class="btn" data-retry-dialogs>Повторить</button>';
    showError(error);
  }
}
async function openDialog(id) {
  var version = ++dialogLoad;
  current = null;
  syncCompose();
  $('#chat').textContent = 'Загружаю диалог…';
  try {
    var dialog = await API.getDialog(id);
    if (version !== dialogLoad) { return; }
    current = dialog;
    $('#chat-title').textContent = current.title;
    $('#chat-context').textContent = 'Изменения с подтверждением';
    renderDialogs();
    renderChat();
    syncCompose();
  } catch (error) {
    if (version !== dialogLoad) { return; }
    $('#chat').innerHTML = '<p class="muted">Не удалось открыть диалог.</p><button class="btn" data-retry-open="' + esc(id) + '">Повторить</button>';
    showError(error);
  }
}
function safeLink(href) {
  // Allow only this application's pages, never model-provided protocols or hosts.
  return /^(overview|tasks|task|schedule|meetings|meeting|new-meeting|notifications|settings)\.html(?:\?[a-zA-Z0-9_=&%:+.\-]*)?$/.test(href || '') ? href : '';
}
function proposalHtml(p) {
  var busy = !!resolving[p.id];
  var html = '<section class="assistant-proposal" aria-label="' + esc(p.title) + '"><div class="row" style="justify-content:space-between;gap:10px"><b>' + esc(p.title) + '</b><span class="badge sm">' + (p.status === 'pending' ? 'На проверке' : p.status === 'applied' ? 'Выполнено' : 'Отклонено') + '</span></div><dl class="proposal-details">';
  (p.details || []).forEach(function (f) {
    html += '<div><dt>' + esc(f.label) + '</dt><dd>' + esc(f.value) + '</dd></div>';
  });
  html += '</dl>';
  if (p.status === 'pending') {
    html += '<div class="proposal-buttons"><button class="btn primary" data-confirm="' + esc(p.id) + '"' + (busy ? ' disabled' : '') + '>' + (busy ? 'Обрабатываю…' : 'Подтвердить') + '</button><button class="btn" data-reject="' + esc(p.id) + '"' + (busy ? ' disabled' : '') + '>Отклонить</button></div>';
  } else if (p.status === 'applied') {
    html += '<p class="proposal-result">' + esc(p.result && p.result.text || 'Действие выполнено') + '</p>';
    var href = safeLink(p.result && p.result.href);
    if (href) { html += '<a class="link" href="' + esc(href) + '">Открыть результат</a>'; }
  } else { html += '<p class="muted">Предложение отклонено</p>'; }
  if (p.error) { html += '<p class="proposal-error" role="alert">' + esc(p.error) + '</p>'; }
  return html + '</section>';
}
function messageHtml(message) {
  if (message.role === 'user') {
    return '<div class="msg-user"><small>' + esc((window.ME_NAME || 'Вы').split(' ')[0]) + '</small><span style="white-space:pre-wrap">' + esc(message.text) + '</span></div>';
  }
  var html = '<div class="msg-ai"><small>Фотопринт AI</small><span style="white-space:pre-wrap">' + esc(message.text) + '</span>';
  (message.proposals || []).forEach(function (p) { html += proposalHtml(p); });
  if (message.source) { html += '<p class="src">Источники: ' + esc(message.source) + '</p>'; }
  if (message.actions && message.actions.length) {
    html += '<div class="proposal-buttons">';
    message.actions.forEach(function (a) { var href = safeLink(a.href); if (href) { html += '<a class="btn" href="' + esc(href) + '">' + esc(a.label) + '</a>'; } });
    html += '</div>';
  }
  return html + '</div>';
}
function renderChat() {
  if (!current) { return; }
  var box = $('#chat');
  if (!current.messages.length) {
    var chips = SUGGESTIONS.map(function (s) { return '<button class="badge purple" data-suggest="' + esc(s) + '">' + esc(s) + '</button>'; }).join('');
    box.innerHTML = '<div class="assistant-empty"><h2>Чем помочь команде сегодня?</h2><p class="muted">Опишите задачу. Я подготовлю предложение, а вы проверите и подтвердите его.</p><div class="assistant-suggestions">' + chips + '</div></div>';
  } else {
    box.innerHTML = current.messages.map(messageHtml).join('');
  }
  if (sending && pendingDialog === current.id) {
    box.insertAdjacentHTML('beforeend', '<div class="msg-ai" id="typing"><small>Фотопринт AI</small><span class="muted">Готовлю ответ и проверяю параметры… это может занять до трёх минут.</span></div>');
  }
  box.scrollTop = box.scrollHeight;
}
async function sendMessage(text) {
  text = text.trim();
  if (!text || sending || !current) { return; }
  if (text.length > 8000) { showError(new Error('Сообщение слишком длинное')); return; }
  var target = current;
  sending = true;
  pendingDialog = target.id;
  $('#input').value = '';
  $('#input').style.height = 'auto';
  target.messages.push({ role: 'user', text: text, at: new Date().toISOString(), temporary: true });
  renderChat();
  syncCompose();
  try {
    await API.sendMessage(target.id, text);
    // Re-read persisted messages so reload and optimistic rendering are identical.
    if (current && current.id === target.id) {
      var saved = await API.getDialog(target.id);
      if (current && current.id === target.id) {
        current = saved;
        $('#chat-title').textContent = current.title;
      }
    }
    await refreshDialogs();
  } catch (error) {
    // A lost response can still have been committed. Reconcile before offering a retry.
    try {
      var fresh = await API.getDialog(target.id);
      if (current && current.id === target.id) { current = fresh; }
    } catch (ignored) {
      target.messages = target.messages.filter(function (m) { return !m.temporary; });
    }
    if (current && current.id === target.id) { $('#input').value = text; }
    showError(error);
  } finally {
    sending = false;
    pendingDialog = null;
    renderChat();
    syncCompose();
  }
}
async function resolveProposal(id, apply) {
  if (!current || resolving[id]) { return; }
  var proposal = null;
  current.messages.forEach(function (m) { (m.proposals || []).forEach(function (p) { if (p.id === id) { proposal = p; } }); });
  if (!proposal || proposal.status !== 'pending') { return; }
  resolving[id] = true;
  delete proposal.error;
  renderChat();
  try {
    var updated = await (apply ? API.confirmAssistantProposal(id) : API.rejectAssistantProposal(id));
    Object.assign(proposal, updated);
  } catch (error) {
    proposal.error = error.message || 'Не удалось выполнить действие. Повторите попытку.';
    showError(error);
  } finally {
    delete resolving[id];
    // The user may have reopened the dialog while the request was running.
    if (current) {
      current.messages.forEach(function (m) { (m.proposals || []).forEach(function (p) { if (p.id === id) { Object.assign(p, proposal); } }); });
    }
    renderChat();
  }
}
document.addEventListener('DOMContentLoaded', async function () {
  syncCompose();
  API.getMe().then(function (me) { window.ME_NAME = me.name; }).catch(function () {});
  $('#context-note').textContent = 'Задачи · график · встречи · команда · уведомления · настройки. История хранится в вашей учётной записи.';
  $('#dialogs').addEventListener('click', function (event) { var item = event.target.closest('.dialog-item'); if (item) { openDialog(item.dataset.id); } });
  $('#new-dialog').addEventListener('click', async function () {
    var button = $('#new-dialog');
    button.disabled = true;
    try { var d = await API.createDialog(); dialogs.unshift(d); await openDialog(d.id); $('#input').focus(); }
    catch (error) { showError(error); }
    finally { button.disabled = false; }
  });
  $('#chat').addEventListener('click', function (event) {
    var chip = event.target.closest('[data-suggest]'); if (chip) { sendMessage(chip.dataset.suggest); return; }
    var confirm = event.target.closest('[data-confirm]'); if (confirm) { resolveProposal(confirm.dataset.confirm, true); return; }
    var reject = event.target.closest('[data-reject]'); if (reject) { resolveProposal(reject.dataset.reject, false); return; }
    var retry = event.target.closest('[data-retry-dialogs]'); if (retry) { loadDialogs(); return; }
    var open = event.target.closest('[data-retry-open]'); if (open) { openDialog(open.dataset.retryOpen); }
  });
  var input = $('#input');
  input.addEventListener('input', function () { syncCompose(); input.style.height = 'auto'; input.style.height = Math.min(input.scrollHeight, 140) + 'px'; });
  input.addEventListener('keydown', function (event) { if (event.key === 'Enter' && !event.shiftKey && !event.isComposing) { event.preventDefault(); sendMessage(input.value); } });
  $('#send').addEventListener('click', function () { sendMessage(input.value); });
  await loadDialogs();
});
