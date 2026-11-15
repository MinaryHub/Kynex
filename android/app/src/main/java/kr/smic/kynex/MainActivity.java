package kr.smic.kynex;

import android.app.Activity;
import android.content.Intent;
import android.net.Uri;
import android.os.Bundle;
import android.view.Gravity;
import android.view.View;
import android.webkit.ValueCallback;
import android.webkit.WebChromeClient;
import android.webkit.WebResourceRequest;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import android.widget.FrameLayout;
import android.widget.TextView;

import java.io.File;
import java.net.HttpURLConnection;
import java.net.ServerSocket;
import java.net.URL;
import java.util.Map;

/**
 * KYNEX on Android: the program itself — the one the desktop runs, built
 * for this device and carried as a native library (libkynex.so) — started
 * here, and its viewer shown in a WebView from 127.0.0.1. The projects are
 * kept in the app's own folder (Android/data/kr.smic.kynex/files/projects,
 * which a file manager or a computer can reach), KYNEX's settings in the
 * app's private one.
 */
public class MainActivity extends Activity {
    private static final int PICK_FILE = 1;

    private Process kynex;
    private WebView view;
    private TextView note;
    private ValueCallback<Uri[]> picking;

    @Override
    protected void onCreate(Bundle saved) {
        super.onCreate(saved);
        FrameLayout root = new FrameLayout(this);
        root.setBackgroundColor(0xff0e1013);
        view = new WebView(this);
        view.setBackgroundColor(0xff0e1013);
        view.setVisibility(View.INVISIBLE);
        note = new TextView(this);
        note.setTextColor(0xffaab2c0);
        note.setTextSize(15);
        note.setGravity(Gravity.CENTER);
        note.setText("KYNEX…");
        root.addView(view, new FrameLayout.LayoutParams(-1, -1));
        root.addView(note, new FrameLayout.LayoutParams(-1, -1));
        // Clear of the status and navigation bars, and of the keyboard.
        root.setFitsSystemWindows(true);
        setContentView(root);

        WebSettings s = view.getSettings();
        s.setJavaScriptEnabled(true);
        s.setDomStorageEnabled(true);
        s.setMediaPlaybackRequiresUserGesture(false);
        s.setSupportZoom(false);
        view.setWebViewClient(new WebViewClient() {
            // The viewer stays here; a link elsewhere — an API key's page,
            // the manual — opens in the browser.
            @Override
            public boolean shouldOverrideUrlLoading(WebView v, WebResourceRequest r) {
                Uri u = r.getUrl();
                if ("127.0.0.1".equals(u.getHost())) return false;
                try {
                    startActivity(new Intent(Intent.ACTION_VIEW, u));
                } catch (Exception e) {
                    // No browser to open it: left alone.
                }
                return true;
            }
        });
        view.setWebChromeClient(new WebChromeClient() {
            // A model or a plan chosen to bring in.
            @Override
            public boolean onShowFileChooser(WebView v, ValueCallback<Uri[]> done, FileChooserParams p) {
                if (picking != null) picking.onReceiveValue(null);
                picking = done;
                Intent pick = new Intent(Intent.ACTION_GET_CONTENT);
                pick.addCategory(Intent.CATEGORY_OPENABLE);
                pick.setType("*/*");
                try {
                    startActivityForResult(pick, PICK_FILE);
                } catch (Exception e) {
                    picking = null;
                    return false;
                }
                return true;
            }
        });
        new Thread(this::start, "kynex").start();
    }

    /** KYNEX started on a port of its own, and its viewer opened once it answers. */
    private void start() {
        try {
            int port;
            try (ServerSocket free = new ServerSocket(0)) {
                port = free.getLocalPort();
            }
            File home = getFilesDir();
            File outside = getExternalFilesDir(null);
            File projects = new File(outside != null ? outside : home, "projects");
            projects.mkdirs();
            File program = new File(getApplicationInfo().nativeLibraryDir, "libkynex.so");
            ProcessBuilder b = new ProcessBuilder(program.getPath(), "view", "--port", String.valueOf(port));
            Map<String, String> env = b.environment();
            env.put("HOME", home.getPath());
            env.put("KYNEX_PROJECTS", projects.getPath());
            env.put("TMPDIR", getCacheDir().getPath());
            b.directory(home);
            b.redirectErrorStream(true);
            b.redirectOutput(new File(getCacheDir(), "kynex.log"));
            kynex = b.start();
            String url = "http://127.0.0.1:" + port + "/";
            for (int tries = 0; tries < 150; tries++) {
                if (!kynex.isAlive()) throw new Exception("KYNEX ended (" + kynex.exitValue() + ")");
                if (answers(url)) {
                    runOnUiThread(() -> {
                        view.loadUrl(url);
                        view.setVisibility(View.VISIBLE);
                        note.setVisibility(View.GONE);
                    });
                    return;
                }
                Thread.sleep(200);
            }
            throw new Exception("KYNEX does not answer");
        } catch (Exception e) {
            runOnUiThread(() -> note.setText("KYNEX could not start:\n" + e.getMessage()));
        }
    }

    private static boolean answers(String url) {
        try {
            HttpURLConnection c = (HttpURLConnection) new URL(url).openConnection();
            c.setConnectTimeout(300);
            c.setReadTimeout(1000);
            int code = c.getResponseCode();
            c.disconnect();
            return code == 200;
        } catch (Exception e) {
            return false;
        }
    }

    @Override
    protected void onActivityResult(int request, int result, Intent data) {
        if (request != PICK_FILE || picking == null) return;
        Uri[] files = result == RESULT_OK && data != null && data.getData() != null ? new Uri[] {data.getData()} : null;
        picking.onReceiveValue(files);
        picking = null;
    }

    @Override
    protected void onDestroy() {
        // KYNEX ends with its window: it saves the project as it goes.
        if (kynex != null) kynex.destroy();
        if (view != null) view.destroy();
        super.onDestroy();
    }
}
