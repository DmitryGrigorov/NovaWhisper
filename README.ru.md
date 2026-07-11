# NovaWhisper

[English](README.md) | **Русский**

Кроссплатформенный голосовой ввод текста. Нажмите глобальную горячую клавишу
в любом приложении, произнесите текст, и обработанная расшифровка будет
вставлена в позицию курсора.

## Демонстрация продукта

![Демонстрация голосового ввода NovaWhisper](assets/ScreenCapture.gif)

NovaWhisper состоит из двух процессов:

1. Python/FastAPI **gateway** распознаёт и обрабатывает речь.
2. Rust/Tauri **desktop-приложение** записывает микрофон, показывает HUD и
   вставляет текст.

Desktop `.exe` не распознаёт речь самостоятельно: gateway должен быть запущен.

## Проверенное оборудование и система

Текущий Windows/CUDA-вариант собран и проверен на следующей конфигурации:

| Компонент | Проверенное значение |
|---|---|
| Операционная система | Windows, версия 25H2, сборка 26200, 64-bit |
| Название в реестре | Windows 10 Pro (Windows может сохранять старое название для новых сборок) |
| Видеокарта | NVIDIA GeForce RTX 3060, 12 ГБ VRAM |
| Драйвер NVIDIA | 610.74 |
| Python | 3.14.6 |
| Rust / Cargo | 1.97.0, toolchain MSVC |
| Локальное распознавание | faster-whisper, мультиязычная `small`, CUDA `float16` |

Основная поддерживаемая Windows-цель — Windows 11. Linux собирается с
ограничениями, указанными ниже; macOS собирается, но пока не тестировалась.

## Установка на чистый Windows-ПК

### 1. Установите необходимые программы

- [Git for Windows](https://git-scm.com/download/win)
- [Python 3.11+](https://www.python.org/downloads/) с опцией **Add Python to PATH**
- [Rust через rustup](https://rustup.rs/) со стандартным MSVC toolchain
- [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
  с workload **Desktop development with C++**
- Свежий NVIDIA Game Ready или Studio Driver для распознавания на CUDA

WebView2 уже входит в Windows 11. Проверьте NVIDIA-драйвер:

```powershell
nvidia-smi
```

### 2. Клонируйте проект и подготовьте gateway

```powershell
git clone https://github.com/DmitryGrigorov/NovaWhisper.git
cd NovaWhisper\server\gateway

py -m venv .venv
.\.venv\Scripts\python.exe -m pip install --upgrade pip
.\.venv\Scripts\pip.exe install -r requirements.txt
.\.venv\Scripts\pip.exe install -r requirements-local.txt
```

`requirements-local.txt` устанавливает faster-whisper, CUDA 12 cuBLAS и
cuDNN 9 DLL внутрь виртуального окружения. Полный CUDA Toolkit обычно не
нужен — достаточно рабочего NVIDIA-драйвера. Gateway автоматически добавляет
каталоги этих DLL на Windows.

### 3. Загрузите модель с поддержкой русского языка

При первом запуске модель скачивается автоматически. Если фоновая загрузка
Hugging Face зависает, заранее скачайте проверенную мультиязычную `small`:

```powershell
$env:HF_HUB_DISABLE_XET = "1"
.\.venv\Scripts\python.exe -c "from huggingface_hub import snapshot_download; print(snapshot_download('Systran/faster-whisper-small'))"
```

`small` поддерживает русский язык и быстро работает на CUDA. Модель
`large-v3-turbo` может дать более высокое качество, но требует больше места,
трафика и времени при первом запуске.

### 4. Запустите CUDA gateway

Откройте PowerShell и не закрывайте его:

```powershell
cd NovaWhisper\server\gateway
$env:HF_HUB_DISABLE_XET = "1"
$env:WHISPR_STT_PROVIDER = "whisper_local"
$env:WHISPR_WHISPER_MODEL = "small"
$env:WHISPR_WHISPER_DEVICE = "cuda"
$env:WHISPR_WHISPER_COMPUTE = "float16"
.\.venv\Scripts\uvicorn.exe app.main:app --host 127.0.0.1 --port 8765
```

Проверьте gateway из другого окна PowerShell:

```powershell
Invoke-RestMethod http://127.0.0.1:8765/healthz
```

Ожидаемый ответ: `status : ok`.

### 5. Соберите и запустите EXE

```powershell
cd NovaWhisper
cargo build --release -p whispr-desktop
.\target\release\whispr-desktop.exe
```

Готовый standalone `.exe` находится здесь:

```text
NovaWhisper\target\release\whispr-desktop.exe
```

Установщик не требуется: `.exe` можно скопировать в удобную папку. При этом
Python gateway всё равно должен быть установлен и запущен. Для разработки
используйте `cargo run -p whispr-desktop`.

### 6. Настройте голосовой ввод

1. Оставьте gateway запущенным.
2. В Settings укажите `ws://127.0.0.1:8765/v1/stream`.
3. Выберите **Russian (Русский)** либо **Auto-detect**.
4. Поставьте курсор в текстовое поле и нажмите настроенную горячую клавишу.
5. Произнесите текст и снова нажмите горячую клавишу для завершения и вставки.
6. Индикатор записи можно перемещать по экрану мышью.

Настройки находятся в `%APPDATA%\whispr\config.json`.

## Запуск на CPU без NVIDIA

```powershell
$env:WHISPR_STT_PROVIDER = "whisper_local"
$env:WHISPR_WHISPER_MODEL = "small"
$env:WHISPR_WHISPER_DEVICE = "cpu"
$env:WHISPR_WHISPER_COMPUTE = "int8"
```

## Быстрая установка на Linux

Установите Python, Rust, WebKitGTK 4.1, GTK 3, appindicator, ALSA, OpenSSL,
`libxdo` и `patchelf`. Затем выполните:

```sh
cd server/gateway
python3 -m venv .venv
./.venv/bin/pip install -r requirements.txt
./.venv/bin/uvicorn app.main:app --host 127.0.0.1 --port 8765

# В другом терминале из корня репозитория
cargo run -p whispr-desktop
```

Глобальная горячая клавиша и вставка текста на Linux сейчас требуют
X11/XWayland.

## Проверка проекта

```powershell
cargo test --workspace
cd server\gateway
.\.venv\Scripts\python.exe -m pytest
```

## Решение проблем

| Симптом | Решение |
|---|---|
| HUD не подключается | Запустите gateway и проверьте порт 8765 и WebSocket URL в Settings. |
| Всегда вставляется одинаковая фраза | Используется `mock`; задайте `WHISPR_STT_PROVIDER=whisper_local`. |
| Не найдена `cublas64_12.dll` или `cudnn64_9.dll` | Повторите `pip install -r requirements-local.txt` и перезапустите gateway. |
| CUDA по-прежнему не работает | Проверьте `nvidia-smi`, обновите драйвер либо используйте CPU-режим. |
| Не найден `link.exe` | Установите Visual Studio Build Tools с Desktop development with C++. |
| Недоступен микрофон | Выберите устройство ввода Windows по умолчанию и закройте приложения с монопольным доступом. |
| Не принимается hotkey | Используйте комбинацию вида `ctrl+shift+space` или `alt+d`. |

## Структура репозитория

| Путь | Назначение |
|---|---|
| `core/` | Аудио, протокол, VAD, конфигурация и streaming client на Rust |
| `platform/insert/` | Вставка текста в другие приложения |
| `apps/desktop/` | Tauri desktop-приложение и интерфейс |
| `server/gateway/` | FastAPI gateway, STT-провайдеры и обработка текста |
| `shared/protocol.md` | Протокол между клиентом и gateway |
| `docs/` | Архитектура, структура и инструкции разработчика |

Дополнительные документы: [архитектура](docs/ARCHITECTURE.md),
[структура проекта](docs/STRUCTURE.md) и
[инструкции разработчика](docs/SKILLS.md).
