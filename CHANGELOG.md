## [unreleased]

### 🐛 Bug Fixes

- *(ci)* Include release version to binaries

### 📚 Documentation

- Add README and MIT license
## [0.4.0] - 2026-08-09

### 🚀 Features

- Spectrogram analysis, phosphor icons, app-state cleanup
- *(signal)* Wiener deconvolution primitive
- *(analysis)* Step response by Wiener deconvolution
- *(ui)* Step Response panel
- *(ui)* Stick input presets for the step response
- *(analysis)* Report the step response as numbers
- *(analysis)* Trim log ends before analysing
- Update logos and use elegance::Button and elegance::Card
- Add remove button for each log + UI improvements

### 🐛 Bug Fixes

- When deselcting either raw or filtered from the plot, it would shuffle states
- Show Auto Tune's placeholder without a log
- Name why an axis has no step response
- *(analysis)* Taper each step response window before the FFT
- *(ui)* No panel goes blank without saying why

### 🚜 Refactor

- Collapse heatmap renderers into one module
- *(parser)* Give FlightData a real interface
- Index per-axis data by Axis, not usize
- Lift the load pipeline out of the UI
- Let the legend own timeseries visibility
- One module per tab
- Bundle the per-sublog analysis
- *(ui)* One tab bar, one heatmap panel

### 📚 Documentation

- Spec the step response metrics follow-up

### ⚡ Performance

- Collapse the Welch views into one spectral pass
- Skip the downsample for legend-hidden series

### 🎨 Styling

- Apply rustfmt
- Derive Psd's Default, reuse the plot-height helper

### ⚙️ Miscellaneous Tasks

- Remove AutoTune Tab
## [0.3.0] - 2026-07-29

### 🚀 Features

- Improving the Parser wrapper
- *(tests)* Add parser tests + fixtures
- *(signal)* Add downsample + moving average functions
- Use of new UI
- Add different tabs for timeseries, filter analysis, pid analysis...

### 💼 Other

- Map blackbox-log create to patch

### ⚙️ Miscellaneous Tasks

- Hide console window on Windows
## [0.2.0] - 2026-05-16

### 🚀 Features

- Add custom icon to the app
- *(parser)* Add RPM filter config parsing and progress callback
- *(analysis)* Add spectral FFT and step response analysis
- *(ui)* Add spectral heatmap and step response panels
- *(app)* Panel switching, progressive log loading, Wayland vsync fix

### ⚙️ Miscellaneous Tasks

- Remove target dir from blackbox-log
## [0.1.0] - 2026-05-15

### 🚀 Features

- Milestone 1 — blackbox viewer

### ⚙️ Miscellaneous Tasks

- Project creation
- Add release workflow for Linux, macOS, Windows
