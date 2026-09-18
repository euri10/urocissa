## Build from Source (Linux Version)

Follow these steps to set up and run the Urocissa app on Linux-based systems. For instructions on setting up the app on Windows, please refer to [this guide](WINDOWS.md).

### 1. Clone the Repository

```bash
git clone https://github.com/hsa00000/urocissa.git
```

This will create a folder called `./urocissa`.

---

### 2. Install Dependencies

Make sure the following software is installed on your system:

- **ffmpeg**: Install via your system's package manager. For Ubuntu, use APT:

  ```bash
  sudo apt update && sudo apt install -y ffmpeg
  ```

  For other Linux distributions, use the appropriate package manager (e.g., `dnf`, `yum`, `pacman`) and find the corresponding package name for installation.

- **Rust**: Install Rust using the official installer:

  ```bash
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  source $HOME/.cargo/env
  ```

- **npm (Node.js)**: Install Node.js (with npm). For Ubuntu, use APT:

  ```bash
  sudo apt install -y nodejs npm
  ```

  For other Linux distributions, use the appropriate package manager (e.g., `dnf`, `yum`, `pacman`) and find the corresponding package name for installation.

---

### 3. Build the Frontend

In the `gallery-frontend` directory, run:

```bash
npm run build
```

---

### 4. Run the Application

Navigate to the `gallery-backend` directory and run the following command to start the app:

```bash
cargo run --release
```

You can now access the app via [http://127.0.0.1:5673](http://127.0.0.1:5673) or [http://127.0.0.1](http://127.0.0.1):\<your_port> if you configured a custom port in Rocket.toml.

---

## Update

### 1. Pull the Latest Changes from the Repository

Navigate to the project directory and pull the latest updates:

```bash
git pull
```

### 2. Rebuild

### Rebuild the Frontend

1. Navigate to the `gallery-frontend` directory:

   ```bash
   cd ./urocissa/gallery-frontend
   ```

2. Build the frontend:

   ```bash
   npm run build
   ```

### Rebuild the Backend

1. Navigate to the `gallery-backend` directory:

   ```bash
   cd ./urocissa/gallery-backend
   ```

2. Build and run the backend using Cargo:

   ```bash
   cargo run --release
   ```

After following these steps, your Urocissa app will be updated to the latest version.

## Backfill grid thumbnails

The fork serves grid JPEGs at a maximum long side of 768 pixels. Existing
viewer JPEGs and their URLs remain unchanged. To create missing grid files,
use the authenticated reindex API with `operations: ["gridThumbnail"]`.
This operation must be requested alone; it is not part of the default reindex.

1. As the owner, obtain a fresh selection snapshot with `POST /get/prefetch`.
   Use the same filters you intend to backfill (`null` for the normal gallery).
2. Send `POST /put/reindex` with the owner authentication and this JSON,
   replacing the example timestamp with the response's `prefetch.timestamp`:

   ```json
   {
     "timestamp": 1789700000000,
     "selection": {"mode": "allExcept", "excludedIndices": []},
     "operations": ["gridThumbnail"]
   }
   ```

   The selection covers that snapshot, not records outside its filters.
3. Monitor `GET /get/reindex/jobs`. Cancel with
   `POST /put/reindex/<jobId>/cancel`. Both require owner authentication.
   Existing regular grid files count as skipped; missing or corrupt viewer
   JPEGs count as failures. Review errors even if the queue has finished.
4. To resume after cancellation, restart, or repaired source files, submit
   a new job with a fresh snapshot. Completed grid files are skipped.

The job uses the existing per-media lock and worker limits. It decodes only
the current viewer JPEG, stages the grid JPEG beside its destination, then
publishes it. It does not rewrite viewer files, bump `cache_version`, or write
media/album metadata. Original media is not required. A non-file grid
destination is reported as an error instead of replaced.

During backfill, the grid endpoint serves the viewer JPEG for missing grid
files with `Cache-Control: no-store`. The worker also honors that policy in
its memory and Cache Storage caches, so subsequent requests can pick up the
small artifact at the same URL. Existing displayed tiles change on their next
load; the backfill does not force a refresh. Normal viewer and completed grid
responses retain their long cache lifetime.
