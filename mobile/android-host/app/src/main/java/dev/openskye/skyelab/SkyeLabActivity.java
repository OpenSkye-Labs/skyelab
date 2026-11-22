package dev.openskye.skyelab;

import android.graphics.Insets;
import android.os.Build;
import android.os.Bundle;
import android.view.View;
import android.view.WindowInsets;

import dev.gpui.mobile.GpuiInputActivity;

/**
 * The launcher activity. GpuiInputActivity (gpui-mobile's NativeActivity with the soft-keyboard bridge, copied
 * from the fetched checkout at build time) does the work; this adds only edge-to-edge keyboard handling.
 */
public class SkyeLabActivity extends GpuiInputActivity {
    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) return; // below API 30 adjustResize still shrinks the view
        // Edge-to-edge (target SDK 35+) stops adjustResize from shrinking NativeActivity's view for the keyboard,
        // so NativeActivity never reports a content-rect change and gpui-mobile never re-reads the insets
        // (mobile/upstream/0003). Padding the content view by the IME height resizes it, which reports the change.
        // GPUI's surface stays full size and takes max(system bars, ime) from WindowInsets, so nothing counts twice.
        View content = findViewById(android.R.id.content);
        content.setOnApplyWindowInsetsListener((view, insets) -> {
            Insets ime = insets.getInsets(WindowInsets.Type.ime());
            if (view.getPaddingBottom() != ime.bottom) view.setPadding(0, 0, 0, ime.bottom);
            return insets;
        });
    }
}
