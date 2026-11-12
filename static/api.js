/* ============================================================
   Слой работы с сервером. Страницы вызывают ТОЛЬКО функции API.*
   Каждая функция возвращает Promise с данными (JSON).
   Ошибка сервера = Promise.reject(Error(сообщение)).
   Формат ответов описан в комментариях над каждой функцией.
   ============================================================ */
var API = {};

function buildQuery(params) {
  var parts = [];
  for (var key in params) {
    if (params[key] !== undefined && params[key] !== null && params[key] !== '') {
      parts.push(encodeURIComponent(key) + '=' + encodeURIComponent(params[key]));
    }
  }
  return parts.length ? '?' + parts.join('&') : '';
}

// Какие запросы уже идут на настоящий бэкенд, даже если USE_MOCK = true
function isRealPath(path) {
  var list = CONFIG.REAL_PATHS || [];
  for (var i = 0; i < list.length; i++) {
    if (path === list[i] || path.indexOf(list[i] + '/') === 0 || path.indexOf(list[i] + '?') === 0) { return true; }
  }
  return false;
}

async function request(method, path, body, timeoutMs) {
  if (CONFIG.USE_MOCK && !isRealPath(path)) {
    return mockRequest(method, path, body);
  }
  var headers = { 'Accept': 'application/json' };
  var options = { method: method, headers: headers, credentials: 'include' };
  if (CONFIG.TOKEN) { headers['Authorization'] = CONFIG.TOKEN; }
  if (body instanceof FormData) {
    options.body = body;                       // файлы отправляются как multipart
  } else if (body !== undefined) {
    headers['Content-Type'] = 'application/json';
    options.body = JSON.stringify(body);
  }
  var controller = timeoutMs ? new AbortController() : null;
  var timer = controller ? setTimeout(function () { controller.abort(); }, timeoutMs) : null;
  if (controller) { options.signal = controller.signal; }
  var response;
  try { response = await fetch(CONFIG.API_BASE + path, options); }
  catch (error) {
    if (error.name === 'AbortError') { throw new Error('Ответ занял слишком много времени. Обновите диалог, прежде чем повторять запрос.'); }
    throw error;
  } finally { if (timer) { clearTimeout(timer); } }
  if (response.status === 401) {
    window.location.href = CONFIG.LOGIN_URL;
    throw new Error('Нужно войти в аккаунт');
  }
  var data = null;
  try { data = await response.json(); } catch (e) { data = null; }
  if (!response.ok) {
    throw new Error((data && (data.message || data.error)) || 'Ошибка сервера (' + response.status + ')');
  }
  return data;
}

/* ---------- Пользователь и справочники ---------- */
// GET /me -> { id, name, initials, role, email }
API.getMe = function () { return request('GET', '/me'); };
// GET /departments -> ['Производство', 'Логистика', ...]
API.getDepartments = function () { return request('GET', '/departments'); };
// GET /employees -> [{ id, name, position, department, role }]
API.getEmployees = function () { return request('GET', '/employees'); };
// POST /auth/login { email, password } -> { token, user }   (сессия хранится в cookie)
API.login = function (email, password) { return request('POST', '/auth/login', { email: email, password: password }); };
// POST /auth/register { name, email, password } -> { token, user }
API.register = function (name, email, password) { return request('POST', '/auth/register', { name: name, email: email, password: password }); };
// POST /auth/logout
API.logout = function () { return request('POST', '/auth/logout', {}); };
// GET /search?q= -> [{ type: 'meeting'|'task', id, title }]
API.search = function (query) { return request('GET', '/search' + buildQuery({ q: query })); };

/* ---------- Обзор ---------- */
// GET /stats -> { meetingsWeek, meetingsDelta, tasksActive, tasksAttention, savedHours }
API.getStats = function () { return request('GET', '/stats'); };
// GET /activity -> [{ id, kind: 'task'|'reminder'|'schedule', title, text, at }]
API.getActivity = function () { return request('GET', '/activity'); };

/* ---------- Встречи ---------- */
// GET /meetings?search&department&period(week|month|all)&status(ready|processing)&limit
//   -> { items: [Meeting], total }
API.getMeetings = function (params) { return request('GET', '/meetings' + buildQuery(params)); };
// GET /meetings/:id -> Meeting (с summary, transcript, participantIds, audioUrl)
API.getMeeting = function (id) { return request('GET', '/meetings/' + id); };
// POST /meetings (multipart: title, date, department, participantIds[], notes, file, actions{}) -> Meeting
API.createMeeting = function (payload) {
  if (CONFIG.USE_MOCK && !isRealPath('/meetings')) { return request('POST', '/meetings', payload); }
  var form = new FormData();
  form.append('title', payload.title);
  form.append('date', payload.date || '');
  form.append('department', payload.department || '');
  form.append('notes', payload.notes || '');
  form.append('participantIds', JSON.stringify(payload.participantIds || []));
  form.append('actions', JSON.stringify(payload.actions || {}));
  if (payload.file) { form.append('file', payload.file); }
  return request('POST', '/meetings', form);
};
// GET /meetings/:id/status -> { status: 'processing'|'ready'|'error', progress: 0..100 }
API.getMeetingStatus = function (id) { return request('GET', '/meetings/' + id + '/status'); };
// POST /meetings/:id/ask { question } -> { answer, source }
API.askAboutMeeting = function (id, question) { return request('POST', '/meetings/' + id + '/ask', { question: question }); };
// GET /meetings/:id/export?format=pdf|docx|txt -> { url }  (в демо: { fileName, content })
API.exportMeeting = function (id, format) { return request('GET', '/meetings/' + id + '/export' + buildQuery({ format: format })); };
// DELETE /meetings/:id
API.deleteMeeting = function (id) { return request('DELETE', '/meetings/' + id); };

/* ---------- Задачи ---------- */
// GET /tasks?status(active|all|done)&assigneeId&sort(due|title|status)&page&pageSize&meetingId
//   -> { items: [Task], total, counts: { active, all, done } }
API.getTasks = function (params) { return request('GET', '/tasks' + buildQuery(params)); };
// GET /tasks/:id -> Task
API.getTask = function (id) { return request('GET', '/tasks/' + id); };
// GET /reminders -> [{ id, time, title, meta, state, tone }]
API.getReminders = function () { return request('GET', '/reminders'); };
// POST /tasks { title, assigneeId, due, priority, description } -> Task
API.createTask = function (payload) { return request('POST', '/tasks', payload); };
// PATCH /tasks/:id { любые поля задачи } -> Task
API.updateTask = function (id, fields) { return request('PATCH', '/tasks/' + id, fields); };
// POST /tasks/:id/comments { text } -> Comment
API.addComment = function (id, text) { return request('POST', '/tasks/' + id + '/comments', { text: text }); };
// PUT /tasks/:id/reminder { before: '15m'|'1h'|'1d'|'0', channel } -> Task (reminder: { at, text })
API.setReminder = function (id, payload) { return request('PUT', '/tasks/' + id + '/reminder', payload); };

/* ---------- График ---------- */
// GET /schedule?weekStart=YYYY-MM-DD
//   -> { weekStart, days:[YYYY-MM-DD], employees:[{id,name,position}],
//        shifts:[{id,employeeId,date,time,status:'confirmed'|'ai'}],
//        proposal: null | { text, count, changes:[{who,date,was,now}] }, published }
API.getSchedule = function (weekStart) { return request('GET', '/schedule' + buildQuery({ weekStart: weekStart })); };
// POST /schedule/shifts { employeeId, date, time:'HH:MM–HH:MM', note } -> Shift
API.addShift = function (payload) { return request('POST', '/schedule/shifts', payload); };
// PUT /schedule/shifts/:id { employeeId, date, time, note } -> Shift
API.updateShift = function (id, payload) { return request('PUT', '/schedule/shifts/' + id, payload); };
// DELETE /schedule/shifts/:id
API.deleteShift = function (id) { return request('DELETE', '/schedule/shifts/' + id); };
// POST /schedule/generate { weekStart } -> { proposal }   (AI составляет предложение)
API.generateSchedule = function (weekStart) { return request('POST', '/schedule/generate', { weekStart: weekStart }); };
// POST /schedule/proposal/reject { weekStart }
API.rejectProposal = function (weekStart) { return request('POST', '/schedule/proposal/reject', { weekStart: weekStart }); };
// POST /schedule/publish { weekStart } -> { published: true }  (утвердить и уведомить сотрудников)
API.publishSchedule = function (weekStart) { return request('POST', '/schedule/publish', { weekStart: weekStart }); };

/* ---------- Уведомления ---------- */
// GET /notifications?filter(all|unread|ai)
//   -> { items:[Notification], counts:{all,unread,ai}, pending: null | {title,text,details,was,now}, nextReminder: null | {time,title,meta} }
API.getNotifications = function (filter) { return request('GET', '/notifications' + buildQuery({ filter: filter })); };
// POST /notifications/read-all
API.readAllNotifications = function () { return request('POST', '/notifications/read-all'); };
// POST /notifications/:id/read
API.readNotification = function (id) { return request('POST', '/notifications/' + id + '/read'); };

/* ---------- Настройки ---------- */
// GET /settings -> Settings (см. mock-data.js)   PUT /settings -> Settings
API.getSettings = function () { return request('GET', '/settings'); };
API.saveSettings = function (settings) { return request('PUT', '/settings', settings); };
// POST /team/invite { email, role } -> Employee
API.inviteMember = function (email, role) { return request('POST', '/team/invite', { email: email, role: role }); };

/* ---------- AI-ассистент ---------- */
// GET /assistant/dialogs -> [{ id, title, context, updatedAt }]
API.getDialogs = function () { return request('GET', '/assistant/dialogs'); };
// GET /assistant/dialogs/:id -> { id, title, context, messages:[{id,role,text,at,steps?,source?,actions?}] }
API.getDialog = function (id) { return request('GET', '/assistant/dialogs/' + id); };
// POST /assistant/dialogs -> Dialog (новый, пустой)
API.createDialog = function () { return request('POST', '/assistant/dialogs', {}); };
// POST /assistant/dialogs/:id/messages { text } -> Message (ответ ассистента)
API.sendMessage = function (id, text) {
  return request('POST', '/assistant/dialogs/' + encodeURIComponent(id) + '/messages', { text: text }, CONFIG.CHAT_TIMEOUT_MS);
};
API.confirmAssistantProposal = function (id) { return request('POST', '/assistant/proposals/' + encodeURIComponent(id) + '/confirm', {}); };
API.rejectAssistantProposal = function (id) { return request('POST', '/assistant/proposals/' + encodeURIComponent(id) + '/reject', {}); };

/* ============================================================
   ДЕМО-СЕРВЕР (работает только при CONFIG.USE_MOCK = true).
   Хранит данные в localStorage, чтобы изменения не пропадали
   при переходе между страницами.
   ============================================================ */
var MOCK_KEY = 'fp_mock_db_v1';

function loadDatabase() {
  var raw = null;
  try { raw = localStorage.getItem(MOCK_KEY); } catch (e) { raw = null; }
  if (raw) { try { return JSON.parse(raw); } catch (e2) { /* упадём на пересоздание */ } }
  var fresh = buildSeed();
  saveDatabase(fresh);
  return fresh;
}
function saveDatabase(database) {
  try { localStorage.setItem(MOCK_KEY, JSON.stringify(database)); } catch (e) { /* ignore */ }
}
function resetMockData() {
  try { localStorage.removeItem(MOCK_KEY); } catch (e) { /* ignore */ }
}
function newMockId(prefix) { return prefix + Date.now().toString(36) + Math.floor(Math.random() * 1000); }
function findById(list, id) {
  for (var i = 0; i < list.length; i++) { if (list[i].id === id) { return list[i]; } }
  return null;
}
function employeeName(database, id) {
  var employee = findById(database.employees, id);
  return employee ? employee.name : '';
}
function withAssigneeName(database, task) {
  var copy = JSON.parse(JSON.stringify(task));
  copy.assigneeName = employeeName(database, task.assigneeId);
  return copy;
}
function dayBounds(weekStart) {
  var days = [];
  var start = new Date(weekStart + 'T12:00:00');
  for (var i = 0; i < 7; i++) {
    var day = new Date(start);
    day.setDate(start.getDate() + i);
    days.push(isoDate(day));
  }
  return days;
}
function buildProposal(database, weekStart) {
  var days = dayBounds(weekStart);
  var changes = [];
  var count = 0;
  for (var i = 0; i < database.shifts.length; i++) {
    var shift = database.shifts[i];
    if (shift.status === 'ai' && days.indexOf(shift.date) >= 0) {
      count++;
      changes.push({ who: employeeName(database, shift.employeeId), date: shift.date, was: shift.was || '', now: shift.time });
    }
  }
  if (count === 0) { return null; }
  return { text: 'AI предлагает усилить вечернюю смену в чт и пт', count: count, changes: changes };
}

function mockHandle(method, path, query, body) {
  var db = loadDatabase();
  var parts = path.split('/').filter(function (p) { return p !== ''; });
  var head = parts[0];
  var id = parts[1];
  var tail = parts[2];
  var i;

  if (head === 'me') { return db.me; }
  if (head === 'employees') { return db.employees; }
  if (head === 'departments') { return ['Производство', 'Логистика', 'Продукт', 'Продажи', 'Маркетинг']; }
  if (head === 'auth') { return { token: 'demo' }; }

  if (head === 'search') {
    var found = [];
    var needle = (query.q || '').toLowerCase();
    for (i = 0; i < db.meetings.length; i++) {
      if (db.meetings[i].title.toLowerCase().indexOf(needle) >= 0) { found.push({ type: 'meeting', id: db.meetings[i].id, title: db.meetings[i].title }); }
    }
    for (i = 0; i < db.tasks.length; i++) {
      if (db.tasks[i].title.toLowerCase().indexOf(needle) >= 0) { found.push({ type: 'task', id: db.tasks[i].id, title: db.tasks[i].title }); }
    }
    return found.slice(0, 8);
  }

  if (head === 'reminders') {
    return [
      { id: 'r1', time: '12:30', title: 'Проверить цветопробу', meta: 'Олегу Волкову · Telegram', state: 'В очереди', tone: 'purple' },
      { id: 'r2', time: '10:18', title: '3 напоминания о задачах', meta: 'Сотрудникам · Telegram и почта', state: 'Доставлены', tone: 'green' },
      { id: 'r3', time: 'После', title: 'Новый график смен', meta: 'Команде · после утверждения', state: 'На паузе', tone: 'orange' }
    ];
  }

  if (head === 'stats') {
    var active = 0;
    var attention = 0;
    for (i = 0; i < db.tasks.length; i++) {
      if (db.tasks[i].status !== 'done') { active++; }
      if (db.tasks[i].status === 'review') { attention++; }
    }
    return { meetingsWeek: db.meetings.length, meetingsDelta: 3, tasksActive: active, tasksAttention: attention, savedHours: 6.5 };
  }

  if (head === 'activity') {
    return [
      { id: 'a1', kind: 'task', title: 'Создано 4 задачи из планёрки', text: 'Ответственные и сроки назначены', at: daysFromToday(0, 10, 24) },
      { id: 'a2', kind: 'reminder', title: 'Отправлены напоминания', text: '3 сотрудника получили уведомления', at: daysFromToday(0, 10, 18) },
      { id: 'a3', kind: 'schedule', title: 'Подготовлено предложение по графику', text: 'Нужно подтверждение руководителя', at: daysFromToday(0, 9, 45) }
    ];
  }

  if (head === 'meetings') {
    if (!id && method === 'GET') {
      var list = [];
      var search = (query.search || '').toLowerCase();
      var limitDate = null;
      if (query.period === 'week') { limitDate = new Date(); limitDate.setDate(limitDate.getDate() - 7); }
      if (query.period === 'month') { limitDate = new Date(); limitDate.setMonth(limitDate.getMonth() - 1); }
      for (i = 0; i < db.meetings.length; i++) {
        var meeting = db.meetings[i];
        if (query.status && meeting.status !== query.status) { continue; }
        if (query.department && meeting.department !== query.department) { continue; }
        if (limitDate && new Date(meeting.date) < limitDate) { continue; }
        if (search && (meeting.title + ' ' + meeting.description).toLowerCase().indexOf(search) < 0) { continue; }
        list.push(meeting);
      }
      list.sort(function (a, b) { return new Date(b.date) - new Date(a.date); });
      var total = list.length;
      if (query.limit) { list = list.slice(0, parseInt(query.limit, 10)); }
      return { items: list, total: total };
    }
    if (!id && method === 'POST') {
      var created = {
        id: newMockId('m'), title: body.title || 'Новая встреча', department: body.department || '', date: body.date ? new Date(body.date).toISOString() : new Date().toISOString(),
        durationMin: 0, participantIds: body.participantIds || [], status: 'processing', progress: 5, tasksCount: 0,
        description: 'Распознаём запись и выделяем договорённости. Можно закрыть страницу — сообщим о готовности.', summary: null, transcript: [], startedAt: Date.now()
      };
      db.meetings.unshift(created);
      saveDatabase(db);
      return created;
    }
    var meetingItem = findById(db.meetings, id);
    if (!meetingItem) { throw new Error('Встреча не найдена'); }
    if (tail === 'status') {
      if (meetingItem.status === 'processing' && meetingItem.startedAt) {
        var progress = Math.min(100, 5 + Math.floor((Date.now() - meetingItem.startedAt) / 120));
        meetingItem.progress = progress;
        if (progress >= 100) {
          meetingItem.status = 'ready';
          meetingItem.durationMin = 20;
          meetingItem.description = 'Саммари готово.';
          meetingItem.summary = { highlight: 'Ассистент подготовил краткое саммари по загруженным материалам.', topics: [], decisions: [], source: 'загруженные материалы.' };
        }
        saveDatabase(db);
      }
      return { status: meetingItem.status, progress: meetingItem.progress };
    }
    if (tail === 'ask') {
      return { answer: 'По материалам встречи «' + meetingItem.title + '»: ' + (meetingItem.summary ? meetingItem.summary.highlight : 'саммари ещё готовится.'), source: 'фрагмент 04:12' };
    }
    if (tail === 'export') {
      return { fileName: meetingItem.title + '.' + (query.format || 'txt'), content: meetingItem.title + '\n\n' + (meetingItem.summary ? meetingItem.summary.highlight : '') };
    }
    if (method === 'DELETE') {
      db.meetings = db.meetings.filter(function (m) { return m.id !== id; });
      saveDatabase(db);
      return { ok: true };
    }
    return meetingItem;
  }

  if (head === 'tasks') {
    if (!id && method === 'GET') {
      var filtered = [];
      var counts = { active: 0, all: db.tasks.length, done: 0 };
      for (i = 0; i < db.tasks.length; i++) {
        var task = db.tasks[i];
        if (task.status === 'done') { counts.done++; } else { counts.active++; }
        if (query.status === 'active' && task.status === 'done') { continue; }
        if (query.status === 'done' && task.status !== 'done') { continue; }
        if (query.assigneeId && task.assigneeId !== query.assigneeId) { continue; }
        if (query.meetingId && task.meetingId !== query.meetingId) { continue; }
        filtered.push(withAssigneeName(db, task));
      }
      var sortKey = query.sort || 'due';
      filtered.sort(function (a, b) {
        if (sortKey === 'title') { return a.title.localeCompare(b.title); }
        if (sortKey === 'status') { return a.status.localeCompare(b.status); }
        return new Date(a.due) - new Date(b.due);
      });
      var pageSize = parseInt(query.pageSize || '5', 10);
      var page = parseInt(query.page || '1', 10);
      return { items: filtered.slice((page - 1) * pageSize, page * pageSize), total: filtered.length, counts: counts };
    }
    if (!id && method === 'POST') {
      var newTask = {
        id: newMockId('t'), code: 'FP-' + (100 + db.tasks.length + 1), title: body.title, assigneeId: body.assigneeId, due: body.due ? new Date(body.due).toISOString() : daysFromToday(1, 12, 0),
        status: 'planned', priority: body.priority || 'Обычный', source: 'Вручную', meetingId: null, description: body.description || '', quote: null, checklist: [], comments: [], reminder: null
      };
      db.tasks.unshift(newTask);
      saveDatabase(db);
      return withAssigneeName(db, newTask);
    }
    var taskItem = findById(db.tasks, id);
    if (!taskItem) { throw new Error('Задача не найдена'); }
    if (tail === 'comments') {
      var comment = { id: newMockId('k'), authorName: db.me.name, isAi: false, text: body.text, at: new Date().toISOString() };
      taskItem.comments.push(comment);
      saveDatabase(db);
      return comment;
    }
    if (tail === 'reminder') {
      var offsets = { '15m': 15, '1h': 60, '1d': 1440, '0': 0 };
      var minutesBefore = offsets[body.before] === undefined ? 60 : offsets[body.before];
      var beforeLabels = { '15m': 'За 15 минут до срока', '1h': 'За час до срока', '1d': 'За день до срока', '0': 'В момент срока' };
      taskItem.reminder = { at: new Date(new Date(taskItem.due).getTime() - minutesBefore * 60000).toISOString(), text: beforeLabels[body.before] + ' · ' + body.channel, before: body.before, channel: body.channel };
      saveDatabase(db);
      return withAssigneeName(db, taskItem);
    }
    if (method === 'PATCH') {
      taskItem.history = taskItem.history || [];
      if (body.status && body.status !== taskItem.status) { taskItem.history.push({ id: newMockId('h'), at: new Date().toISOString(), text: 'Статус изменён: ' + STATUS[body.status].label }); }
      if (body.assigneeId && body.assigneeId !== taskItem.assigneeId) { taskItem.history.push({ id: newMockId('h'), at: new Date().toISOString(), text: 'Ответственный: ' + employeeName(db, body.assigneeId) }); }
      for (var field in body) { taskItem[field] = body[field]; }
      saveDatabase(db);
    }
    return withAssigneeName(db, taskItem);
  }

  if (head === 'schedule') {
    var weekStart = (body && body.weekStart) || query.weekStart || db.weekDays[0];
    if (!id && method === 'GET') {
      var days = dayBounds(weekStart);
      var weekShifts = [];
      for (i = 0; i < db.shifts.length; i++) { if (days.indexOf(db.shifts[i].date) >= 0) { weekShifts.push(db.shifts[i]); } }
      return {
        weekStart: weekStart, days: days, employees: db.employees, shifts: weekShifts, proposal: buildProposal(db, weekStart), published: db.published,
        why: { text: 'На планёрке ожидаемый рост заказов оценили в 20%. Вечерняя смена поможет распределить нагрузку. Дневной выпуск закроет дежурная команда.', source: 'планёрка производства · 04:12', href: 'meeting.html?id=m1' },
        checks: ['Пересечений смен не найдено', 'Между сменами — не менее 12 часов']
      };
    }
    if (id === 'shifts') {
      if (method === 'POST') {
        var shift = { id: newMockId('s'), employeeId: body.employeeId, date: body.date, time: body.time, note: body.note || '', status: 'confirmed' };
        db.shifts.push(shift);
        saveDatabase(db);
        return shift;
      }
      var shiftItem = findById(db.shifts, tail);
      if (!shiftItem) { throw new Error('Смена не найдена'); }
      if (method === 'DELETE') { db.shifts = db.shifts.filter(function (s) { return s.id !== tail; }); saveDatabase(db); return { ok: true }; }
      shiftItem.employeeId = body.employeeId; shiftItem.date = body.date; shiftItem.time = body.time; shiftItem.note = body.note || ''; shiftItem.status = 'confirmed';
      saveDatabase(db);
      return shiftItem;
    }
    if (id === 'generate') {
      for (i = 0; i < db.shifts.length; i++) {
        if (db.shifts[i].status === 'confirmed' && (db.shifts[i].employeeId === 'e2' || db.shifts[i].employeeId === 'e3') && db.shifts[i].date === dayBounds(weekStart)[3]) {
          db.shifts[i].was = db.shifts[i].time; db.shifts[i].status = 'ai'; db.shifts[i].time = '14:00–22:00';
        }
      }
      db.published = false;
      saveDatabase(db);
      return { proposal: buildProposal(db, weekStart) };
    }
    if (id === 'proposal') {
      for (i = 0; i < db.shifts.length; i++) {
        if (db.shifts[i].status === 'ai') { db.shifts[i].status = 'confirmed'; db.shifts[i].time = db.shifts[i].was || db.shifts[i].time; }
      }
      saveDatabase(db);
      return { ok: true };
    }
    if (id === 'publish') {
      for (i = 0; i < db.shifts.length; i++) { if (db.shifts[i].status === 'ai') { db.shifts[i].status = 'confirmed'; } }
      db.published = true;
      saveDatabase(db);
      return { published: true };
    }
  }

  if (head === 'notifications') {
    if (id === 'read-all') { for (i = 0; i < db.notifications.length; i++) { db.notifications[i].read = true; } saveDatabase(db); return { ok: true }; }
    if (id) { var notice = findById(db.notifications, id); if (notice) { notice.read = true; saveDatabase(db); } return { ok: true }; }
    var items = [];
    var noticeCounts = { all: db.notifications.length, unread: 0, ai: 0 };
    for (i = 0; i < db.notifications.length; i++) {
      var current = db.notifications[i];
      if (!current.read) { noticeCounts.unread++; }
      if (current.fromAi) { noticeCounts.ai++; }
      if (query.filter === 'unread' && current.read) { continue; }
      if (query.filter === 'ai' && !current.fromAi) { continue; }
      items.push(current);
    }
    var pending = buildProposal(db, db.weekDays[0]);
    return {
      items: items, counts: noticeCounts,
      pending: pending ? { title: 'Изменения графика', text: 'Четверг и пятница, 12–13 ноября', who: 'Иван Петров · Олег Волков', was: '08:00–16:00', now: '14:00–22:00' } : null,
      nextReminder: { time: '12:30', title: 'Проверить цветопробу', meta: 'Олег Волков · Telegram' }
    };
  }

  if (head === 'settings') {
    if (method === 'PUT') { db.settings = body; saveDatabase(db); }
    return db.settings;
  }
  if (head === 'team') {
    var member = { id: newMockId('e'), name: body.email, position: '', role: body.role, department: '' };
    db.employees.push(member);
    saveDatabase(db);
    return member;
  }

  if (head === 'assistant') {
    var dialogId = parts[2];
    var dialogTail = parts[3];
    if (!dialogId && method === 'GET') {
      var dialogList = [];
      for (i = 0; i < db.dialogs.length; i++) { dialogList.push({ id: db.dialogs[i].id, title: db.dialogs[i].title, context: db.dialogs[i].context, updatedAt: db.dialogs[i].updatedAt }); }
      dialogList.sort(function (a, b) { return new Date(b.updatedAt) - new Date(a.updatedAt); });
      return dialogList;
    }
    if (!dialogId && method === 'POST') {
      var dialog = { id: newMockId('d'), title: 'Новый диалог', context: 'Общий вопрос', updatedAt: new Date().toISOString(), messages: [] };
      db.dialogs.unshift(dialog);
      saveDatabase(db);
      return dialog;
    }
    var dialogItem = findById(db.dialogs, dialogId);
    if (!dialogItem) { throw new Error('Диалог не найден'); }
    if (dialogTail === 'messages') {
      dialogItem.messages.push({ id: newMockId('dm'), role: 'user', text: body.text, at: new Date().toISOString() });
      if (dialogItem.title === 'Новый диалог') { dialogItem.title = body.text.slice(0, 40); }
      var reply = { id: newMockId('dm'), role: 'assistant', text: body.reply || ('Принял запрос. Сейчас в рабочем пространстве ' + db.meetings.length + ' встреч и ' + db.tasks.length + ' задач — подготовлю ответ по ним.'), at: new Date().toISOString() };
      if (body.source) { reply.source = body.source; }
      dialogItem.messages.push(reply);
      dialogItem.updatedAt = reply.at;
      saveDatabase(db);
      return reply;
    }
    return dialogItem;
  }

  throw new Error('Неизвестный запрос: ' + method + ' ' + path);
}

function mockRequest(method, path, body) {
  return new Promise(function (resolve, reject) {
    setTimeout(function () {
      try {
        var questionIndex = path.indexOf('?');
        var cleanPath = questionIndex >= 0 ? path.slice(0, questionIndex) : path;
        var query = {};
        if (questionIndex >= 0) {
          var pairs = path.slice(questionIndex + 1).split('&');
          for (var i = 0; i < pairs.length; i++) {
            var pair = pairs[i].split('=');
            query[decodeURIComponent(pair[0])] = decodeURIComponent(pair[1] || '');
          }
        }
        resolve(JSON.parse(JSON.stringify(mockHandle(method, cleanPath, query, body || {}))));
      } catch (error) {
        reject(error);
      }
    }, CONFIG.MOCK_DELAY_MS);
  });
}

/* Explicit deletions; owner/role checks are performed by the Rust server. */
API.deleteSummary = function (id) { return request('DELETE', '/meetings/' + encodeURIComponent(id) + '/summary'); };
API.deleteTask = function (id) { return request('DELETE', '/tasks/' + encodeURIComponent(id)); };
API.deleteDialog = function (id) { return request('DELETE', '/assistant/dialogs/' + encodeURIComponent(id)); };
API.deleteAllDialogs = function () { return request('DELETE', '/assistant/dialogs'); };
API.deleteNotification = function (id) { return request('DELETE', '/notifications/' + encodeURIComponent(id)); };
API.deleteAllNotifications = function () { return request('DELETE', '/notifications'); };
