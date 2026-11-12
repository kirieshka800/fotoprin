# Бэкенд: «Задачи», «График работы», «Уведомления»

Запуск: `cargo run` — миграция 006 применится сама.
Фронт: `static/config.js` → `REAL_SECTIONS` — какие разделы ходят в реальный API (остальное пока демо).

## Эндпоинты (контракт — static/api.js)
- Задачи: `GET /api/tasks` (status, assigneeId, sort, page, pageSize, meetingId), `POST /api/tasks`, `GET|PATCH|DELETE /api/tasks/:id`, `POST /api/tasks/:id/comments`, `PUT /api/tasks/:id/reminder`, `GET /api/reminders`.
- График (запись — только руководитель): `GET /api/schedule?weekStart=`, `POST /api/schedule/shifts`, `PUT|DELETE /api/schedule/shifts/:id`, `POST /api/schedule/generate` (Ollama), `POST /api/schedule/proposal/reject`, `POST /api/schedule/publish`.
- Уведомления: `GET /api/notifications?filter=all|unread|ai`, `POST /api/notifications/read-all`, `POST /api/notifications/:id/read`.

## Как это работает
- `POST /api/meetings/confirm` теперь создаёт настоящие задачи из реестра нейросети (ответственный по имени, срок разбирается из текста; если не распознан — пометка в описании), автонапоминание за час и уведомления.
- «Составить с AI»: промпт из сотрудников, текущего графика и протоколов последних встреч → Ollama → каждое изменение проверяется (сотрудник, дата в неделе, отдых ≥ 12 ч) → смены со статусом `ai`. Публикация уведомляет сотрудников.
- Воркер раз в 30 с доставляет сработавшие напоминания (учитывает тихие часы и `sendReminders`) — пока как уведомление в приложении.
- Пользователь: берётся из вашей авторизации (users); для него автоматически создаётся запись в employees (по почте или имени).
- Тесты чистой логики: `cargo test` (src/util.rs).

## AI-ассистент: реальные диалоги и подтверждение

Основная логика — `src/assistant/` (Rust), интерфейс — `static/assistant.js`. Раздел `/assistant` добавлен в `REAL_PATHS`; он больше не использует демо-диалоги из localStorage.

- `GET /api/assistant/dialogs` — собственные диалоги (до 200 последних).
- `POST /api/assistant/dialogs {}` — новый диалог.
- `GET /api/assistant/dialogs/:id` — последние 300 сообщений, предложения и сохранённые результаты.
- `POST /api/assistant/dialogs/:id/messages {"text":"..."}` — ответ и предложения, **без изменения рабочих данных**.
- `POST /api/assistant/proposals/:id/confirm {}` — проверить владельца, актуальные права, неизменность просмотренных записей и выполнить сохранённое действие.
- `POST /api/assistant/proposals/:id/reject {}` — отклонить предложение.

Сообщение: `{id,role,text,at,source,actions:[{label,href}],proposals:[{id,title,details:[{label,value}],status,result,kind}]}`. При `applied` результат содержит текст и безопасную ссылку; после отказа статус `rejected`. Клиент отправляет только ID: менять параметры на подтверждении нельзя.

Проверки и изменения выполняются на Rust. Обновление данных, история, уведомления и переход предложения в `applied` сохраняются одной транзакцией. Краткая блокировка записываемых таблиц на этапе подтверждения защищает сравнение снимков и от вставок через старые API; чтение остаётся доступным. Параллельные подтверждения сериализуются по ID предложения. Во время ожидания модели соединение PostgreSQL не удерживается: используется ограниченная по времени аренда диалога.

Полный запуск и ограничения описаны в README.md. Настоящую отправку в почту/Telegram эта доработка не добавляет.
