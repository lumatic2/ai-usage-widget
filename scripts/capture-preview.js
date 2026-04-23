const { app, BrowserWindow, ipcMain } = require('electron');
const path = require('path');
const fs = require('fs');

async function main() {
  await app.whenReady();

  ipcMain.handle('widget:get-initial-state', async () => ({
    planType: 'CODEX',
    primary: { usedPercent: 7, resetAfterSeconds: 17640 },
    secondary: { usedPercent: 4, resetAfterSeconds: 55260 },
    sessionLabel: 'preview',
    claude: {
      isConfigured: true,
      needsLogin: false,
      primary: { usedPercent: 42, resetAfterSeconds: 9240 },
      secondary: { usedPercent: 18, resetAfterSeconds: 48600 }
    }
  }));

  const outputDir = path.join(__dirname, '..', 'assets');
  fs.mkdirSync(outputDir, { recursive: true });
  const outputPath = path.join(outputDir, 'widget-screenshot.png');

  const win = new BrowserWindow({
    width: 680,
    height: 260,
    show: false,
    frame: false,
    transparent: true,
    backgroundColor: '#00000000',
    webPreferences: {
      preload: path.join(__dirname, '..', 'preload.js')
    }
  });

  await win.loadFile(path.join(__dirname, '..', 'renderer', 'index.html'));
  await new Promise((resolve) => setTimeout(resolve, 1200));
  const image = await win.capturePage();
  fs.writeFileSync(outputPath, image.toPNG());
  console.log(outputPath);

  await win.close();
  app.quit();
}

main().catch((error) => {
  console.error(error);
  app.exit(1);
});
