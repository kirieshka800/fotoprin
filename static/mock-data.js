/* ============================================================
   ДЕМО-ДАННЫЕ. Используются только когда CONFIG.USE_MOCK = true.
   Когда подключите бэк — файл можно удалить (и убрать его <script>).
   ============================================================ */
function daysFromToday(offsetDays, hours, minutes) {
  var date = new Date();
  date.setDate(date.getDate() + offsetDays);
  date.setHours(hours, minutes || 0, 0, 0);
  return date.toISOString();
}

function mondayOfCurrentWeek() {
  var date = new Date();
  var weekday = date.getDay();
  var shift = weekday === 0 ? -6 : 1 - weekday;
  date.setDate(date.getDate() + shift);
  return date;
}

function buildSeed() {
  var monday = mondayOfCurrentWeek();
  var weekDays = [];
  for (var d = 0; d < 7; d++) {
    var day = new Date(monday);
    day.setDate(monday.getDate() + d);
    weekDays.push(isoDate(day));
  }

  var employees = [
    { id: 'e1', name: 'Анна Смирнова', position: 'Руководитель', role: 'Руководитель', department: 'Производство' },
    { id: 'e2', name: 'Иван Петров', position: 'Оператор печати', role: 'Сотрудник', department: 'Производство' },
    { id: 'e3', name: 'Олег Волков', position: 'Оператор печати', role: 'Сотрудник', department: 'Производство' },
    { id: 'e4', name: 'Мария Орлова', position: 'Отдел продаж', role: 'Сотрудник', department: 'Продажи' },
    { id: 'e5', name: 'Елена Ким', position: 'Отдел продаж', role: 'Сотрудник', department: 'Продажи' }
  ];

  var meetings = [
    {
      id: 'm1', title: 'Планёрка производства', department: 'Производство', date: daysFromToday(0, 10, 0), durationMin: 32,
      participantIds: ['e1', 'e2', 'e3', 'e4', 'e5'], status: 'ready', progress: 100, tasksCount: 4,
      description: 'Подготовка к сезонному пику, тестовый тираж и усиление вечерней смены.',
      summary: {
        highlight: 'Готовим производство к сезонному пику: переносим часть заказов на вторую смену и завершаем тест новой фотокниги до пятницы.',
        topics: [
          { title: 'Загрузка печатного цеха', text: 'Во второй половине недели ожидается рост заказов на 20%.', time: '04:12' },
          { title: 'Новая линейка фотокниг', text: 'Тестовый тираж — 50 экземпляров. Проверяем цвет и переплёт.', time: '12:40' },
          { title: 'Сроки и клиентский сервис', text: 'Приоритет — срочные заказы. Продажи обновят сроки на сайте.', time: '23:08' }
        ],
        decisions: ['Усилить вечернюю смену в четверг и пятницу.', 'Согласовать тестовый тираж до 13 ноября.'],
        source: 'запись встречи.'
      },
      audioUrl: '', audioDuration: 1934,
      transcript: [
        { time: '00:12', speaker: 'Анна Смирнова', text: 'Коллеги, начинаем. Главная тема — подготовка к сезонному пику.' },
        { time: '04:12', speaker: 'Иван Петров', text: 'На четверг и пятницу нужно усилить вечернюю смену, заказов будет больше на 20%.' },
        { time: '12:40', speaker: 'Олег Волков', text: 'Тестовый тираж фотокниги — 50 экземпляров, проверим цвет и переплёт.' }
      ]
    },
    { id: 'm2', title: 'Логистика и отгрузки', department: 'Логистика', date: daysFromToday(0, 9, 0), durationMin: 28, participantIds: ['e1', 'e2', 'e3', 'e4'],
      status: 'processing', progress: 65, tasksCount: 0, description: 'Распознаём запись и выделяем договорённости. Можно закрыть страницу — сообщим о готовности.', summary: null, transcript: [] },
    { id: 'm3', title: 'Запуск новой фотокниги', department: 'Продукт', date: daysFromToday(-1, 15, 30), durationMin: 45, participantIds: ['e1', 'e2', 'e4', 'e5'], status: 'ready', progress: 100, tasksCount: 3,
      description: 'Согласовали позиционирование, материалы и дату тестового запуска.', summary: { highlight: 'Определили дату тестового запуска и состав материалов.', topics: [], decisions: [], source: 'запись встречи.' }, transcript: [] },
    { id: 'm4', title: 'Синхронизация отдела продаж', department: 'Продажи', date: daysFromToday(-2, 11, 0), durationMin: 25, participantIds: ['e1', 'e2', 'e3', 'e4', 'e5', 'e1'], status: 'ready', progress: 100, tasksCount: 5,
      description: 'Обновление прогноза заказов и приоритетов по корпоративным клиентам.', summary: { highlight: 'Обновили прогноз и приоритеты.', topics: [], decisions: [], source: 'запись встречи.' }, transcript: [] },
    { id: 'm5', title: 'Качество печати: разбор недели', department: 'Производство', date: daysFromToday(-2, 9, 30), durationMin: 40, participantIds: ['e1', 'e2', 'e3'], status: 'ready', progress: 100, tasksCount: 2,
      description: 'Зафиксировали критерии приёмки и план проверки цветопроб перед тиражом.', summary: { highlight: 'Критерии приёмки зафиксированы.', topics: [], decisions: [], source: 'запись встречи.' }, transcript: [] },
    { id: 'm6', title: 'Планирование промокампании', department: 'Маркетинг', date: daysFromToday(-3, 14, 0), durationMin: 35, participantIds: ['e1', 'e4', 'e5', 'e2'], status: 'ready', progress: 100, tasksCount: 3,
      description: 'Подготовка сезонной акции: баннеры, рассылка и единый календарь публикаций.', summary: { highlight: 'Календарь публикаций согласован.', topics: [], decisions: [], source: 'запись встречи.' }, transcript: [] },
    { id: 'm7', title: 'Планёрка склада', department: 'Логистика', date: daysFromToday(-5, 10, 0), durationMin: 20, participantIds: ['e1', 'e3'], status: 'ready', progress: 100, tasksCount: 1,
      description: 'Остатки бумаги и график поставок.', summary: { highlight: 'Остатки бумаги в норме.', topics: [], decisions: [], source: 'запись встречи.' }, transcript: [] },
    { id: 'm8', title: 'Маркетинг: итоги месяца', department: 'Маркетинг', date: daysFromToday(-6, 16, 0), durationMin: 30, participantIds: ['e1', 'e4'], status: 'ready', progress: 100, tasksCount: 2,
      description: 'Результаты рассылок и план на следующий месяц.', summary: { highlight: 'Рассылки показали рост.', topics: [], decisions: [], source: 'запись встречи.' }, transcript: [] }
  ];

  var tasks = [
    { id: 't1', code: 'FP-104', title: 'Согласовать график смен', assigneeId: 'e1', due: daysFromToday(0, 16, 0), status: 'review', priority: 'Обычный', source: 'Планёрка', meetingId: 'm1',
      description: 'Проверить предложенные смены на 12–13 ноября. Учесть рост заказов и доступность операторов. После согласования опубликовать график и уведомить сотрудников.',
      quote: { title: 'Планёрка производства · фрагмент 04:12', text: '«На четверг и пятницу нужно усилить вечернюю смену»' },
      checklist: [{ id: 'c1', text: 'Проверить загрузку печатного цеха', done: true }, { id: 'c2', text: 'Сверить доступность Ивана и Олега', done: true }, { id: 'c3', text: 'Утвердить и опубликовать график', done: false }],
      comments: [
        { id: 'k1', authorName: 'Фотопринт AI', isAi: true, text: 'Создал задачу из встречи, назначил Анну Смирнову и установил срок: сегодня, 16:00.', at: daysFromToday(0, 10, 24) },
        { id: 'k2', authorName: 'Анна Смирнова', isAi: false, text: 'Загрузку проверила. Осталось утвердить график.', at: daysFromToday(0, 10, 35) }
      ],
      reminder: { at: daysFromToday(0, 15, 0), text: 'За час до срока · Telegram и почта', before: '1h', channel: 'Telegram и почта' },
      scheduleLink: { text: '2 сотрудника · 4 смены. Изменения ещё не опубликованы.' } },
    { id: 't2', code: 'FP-105', title: 'Проверить цветопробу', assigneeId: 'e3', due: daysFromToday(0, 18, 0), status: 'in_progress', priority: 'Обычный', source: 'Планёрка', meetingId: 'm1', description: '', quote: null, checklist: [], comments: [], reminder: null },
    { id: 't3', code: 'FP-106', title: 'Подготовить тестовый тираж', assigneeId: 'e2', due: daysFromToday(1, 12, 0), status: 'in_progress', priority: 'Обычный', source: 'Планёрка', meetingId: 'm1', description: '50 экземпляров · проверить цвет и переплёт.', quote: null, checklist: [], comments: [], reminder: null },
    { id: 't4', code: 'FP-107', title: 'Обновить сроки на сайте', assigneeId: 'e4', due: daysFromToday(2, 10, 0), status: 'planned', priority: 'Обычный', source: 'Планёрка', meetingId: 'm1', description: '', quote: null, checklist: [], comments: [], reminder: null },
    { id: 't5', code: 'FP-108', title: 'Собрать прогноз заказов', assigneeId: 'e5', due: daysFromToday(2, 15, 0), status: 'in_progress', priority: 'Обычный', source: 'Продажи', meetingId: 'm4', description: '', quote: null, checklist: [], comments: [], reminder: null },
    { id: 't6', code: 'FP-109', title: 'Обновить список корпоративных клиентов', assigneeId: 'e5', due: daysFromToday(-2, 12, 0), status: 'done', priority: 'Обычный', source: 'Продажи', meetingId: 'm4', description: '', quote: null, checklist: [], comments: [], reminder: null }
  ];
  // «лишние» задачи, чтобы работала пагинация
  var extraTitles = ['Согласовать макет с клиентом', 'Заказать бумагу', 'Проверить остатки плёнки', 'Настроить рассылку', 'Подготовить акт приёмки', 'Обновить прайс-лист', 'Провести инвентаризацию'];
  for (var i = 0; i < extraTitles.length; i++) {
    tasks.push({ id: 'tx' + i, code: 'FP-' + (110 + i), title: extraTitles[i], assigneeId: employees[i % 5].id, due: daysFromToday(3 + i, 12, 0), status: 'in_progress', priority: 'Обычный', source: 'Встреча', meetingId: 'm3', description: '', quote: null, checklist: [], comments: [], reminder: null });
  }

  var shifts = [];
  var pattern = { e2: '08:00–16:00', e3: '08:00–16:00', e4: '09:00–18:00', e5: '09:00–18:00', e1: '09:00–18:00' };
  for (var key in pattern) {
    for (var w = 0; w < 5; w++) {
      var time = pattern[key];
      var status = 'confirmed';
      var wasTime = time;
      if ((key === 'e2' || key === 'e3') && w >= 3) { time = '14:00–22:00'; status = 'ai'; }
      shifts.push({ id: 's-' + key + '-' + w, employeeId: key, date: weekDays[w], time: time, status: status, was: status === 'ai' ? wasTime : '' });
    }
  }

  var notifications = [
    { id: 'n1', at: daysFromToday(0, 10, 24), title: 'Созданы 4 задачи из планёрки', text: 'У каждой — ответственный и срок. Проверьте результат перед началом работы.', category: 'Задачи', actionLabel: 'Открыть задачи', href: 'tasks.html', read: false, fromAi: true },
    { id: 'n2', at: daysFromToday(0, 10, 18), title: 'Напоминания доставлены', text: '3 сотрудника получили напоминания в Telegram и по электронной почте.', category: 'Напоминания', actionLabel: 'Посмотреть историю', href: 'tasks.html', read: false, fromAi: true },
    { id: 'n3', at: daysFromToday(0, 9, 45), title: 'График смен ждёт решения', text: 'AI предлагает перераспределить смены Ивана Петрова и Олега Волкова.', category: 'График', actionLabel: 'Посмотреть изменения', href: 'schedule.html', read: false, fromAi: true },
    { id: 'n4', at: daysFromToday(-1, 17, 0), title: 'Готово саммари встречи', text: 'Запуск новой фотокниги: ключевые решения и 3 задачи уже доступны.', category: 'Встречи', actionLabel: 'Открыть саммари', href: 'meeting.html?id=m3', read: true, fromAi: true },
    { id: 'n5', at: daysFromToday(-2, 12, 0), title: 'Задача завершена', text: 'Елена Ким обновила список корпоративных клиентов.', category: 'Задачи', actionLabel: 'Подробнее', href: 'task.html?id=t6', read: true, fromAi: false }
  ];

  var settings = {
    company: 'Фотопринт', timezone: 'Москва · UTC+3',
    autonomous: true, createTasks: true, sendReminders: true, publishRule: 'Только после подтверждения',
    telegram: { connected: true, enabled: true }, email: { connected: true, enabled: true, address: 'anna@photoprint.example' },
    quietHours: { enabled: true, from: '20:00', to: '09:00' }
  };

  var dialogs = [
    { id: 'd1', title: 'План на неделю', context: 'Производство', updatedAt: daysFromToday(0, 10, 40), messages: [
      { id: 'dm1', role: 'user', text: 'Составь план по итогам планёрки и учти загрузку печатного цеха.', at: daysFromToday(0, 10, 40) },
      { id: 'dm2', role: 'assistant', text: 'Собрал план из договорённостей. Для сезонного пика предлагаю перераспределить операторов на четверг и пятницу.', at: daysFromToday(0, 10, 41),
        steps: [
          { title: 'Утвердить график смен', meta: 'Анна Смирнова · сегодня, 16:00', badge: 'Нужно решение', tone: 'orange' },
          { title: 'Подготовить тестовый тираж', meta: 'Иван Петров · завтра, 12:00', badge: 'Задача создана', tone: 'green' },
          { title: 'Обновить сроки на сайте', meta: 'Мария Орлова · послезавтра, 10:00', badge: 'Задача создана', tone: 'green' }
        ], source: 'планёрка производства · 04:12',
        actions: [{ label: 'Посмотреть график', href: 'schedule.html', primary: true }, { label: 'Открыть 4 задачи', href: 'tasks.html', primary: false }] }
    ] },
    { id: 'd2', title: 'Риски по заказам', context: 'Общий вопрос', updatedAt: daysFromToday(0, 9, 15), messages: [] },
    { id: 'd3', title: 'Итоги запуска фотокниги', context: 'Маркетинг', updatedAt: daysFromToday(-1, 16, 10), messages: [] },
    { id: 'd4', title: 'Кто свободен в пятницу?', context: 'График', updatedAt: daysFromToday(-1, 14, 32), messages: [] }
  ];

  return { me: { id: 'e1', name: 'Анна Смирнова', initials: 'АС', role: 'Руководитель команды', email: 'anna@photoprint.example' },
    employees: employees, meetings: meetings, tasks: tasks, shifts: shifts, weekDays: weekDays,
    notifications: notifications, settings: settings, dialogs: dialogs, published: false };
}
