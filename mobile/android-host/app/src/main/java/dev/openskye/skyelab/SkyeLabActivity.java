package dev.openskye.skyelab;

import android.graphics.Insets;
import android.os.Build;
import android.os.Bundle;
import android.view.View;
import android.view.WindowInsets;

import dev.gpui.mobile.GpuiInputActivity;

/** GpuiInputActivity (copied from gpui-mobile at build time) plus edge-to-edge keyboard insets. */
public class SkyeLabActivity extends GpuiInputActivity {
    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) return; // WindowInsets.Type is API 30
        // Edge-to-edge (target SDK 35+) stops adjustResize from shrinking NativeActivity's view for the keyboard,
        // so NativeActivity never reports a content-rect change and gpui-mobile never re-reads the insets
        // (mobile/upstream/0003). Padding the content view by the IME height resizes it, which reports the change.
        // The surface stays full size, and mobile/src/app.rs pads by insets().effective() = max(bars, ime), so the
        // padding here is never counted twice.
        View content = findViewById(android.R.id.content);
        content.setOnApplyWindowInsetsListener((view, insets) -> {
            Insets ime = insets.getInsets(WindowInsets.Type.ime());
            if (view.getPaddingBottom() != ime.bottom) view.setPadding(0, 0, 0, ime.bottom);
            return insets;
        });
    }
}
