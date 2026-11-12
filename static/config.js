/* ============================================================
   Настройки подключения к бэкенду.
   Рабочие разделы перечислены в REAL_PATHS и используют Rust API.
   USE_MOCK оставлен для ещё не подключённых вспомогательных функций.
   Чтобы подключить настоящий бэк:
     1) USE_MOCK = false
     2) API_BASE = адрес вашего API (например '/api' или 'https://site.ru/api')
     3) При необходимости TOKEN — он уйдёт в заголовок Authorization.
   Список всех запросов — в файле api.js (функции API.*).
   ============================================================ */
var CONFIG = {
  USE_MOCK: true,
  // Если страницы открыты через сам Rust-сервер (http://127.0.0.1:8081/...), адрес относительный.
  API_BASE: (window.location.protocol === 'file:') ? 'http://127.0.0.1:8081/api' : '/api',

  // Диалоги, предложения и подтверждения ассистента хранятся на Rust-сервере.
  // Реальные разделы, включая диалоги и действия ассистента.
  REAL_PATHS: ['/auth', '/me', '/settings', '/stats', '/activity', '/employees', '/departments', '/meetings', '/tasks', '/reminders', '/schedule', '/notifications', '/search', '/assistant'],
  CHAT_TIMEOUT_MS: 210000,       // модель 7B может отвечать долго
  TOKEN: null,
  MOCK_DELAY_MS: 250,
  LOGIN_URL: 'login.html'
};
