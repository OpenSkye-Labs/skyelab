#!/bin/sh
# A fake adb for tests and live checks: one booted emulator (emulator-5554, AVD Fake_Pixel, Android 16).
# Copy it into a folder of its own: it logs each call ("[arg][arg]…") to fake-adb.log next to itself, and
# the env it ran with to fake-adb.env. Its screen is blue until an `input` call, then flips between blue and green.
dir=$(dirname "$0")
{ for a in "$@"; do printf '[%s]' "$a"; done; echo; } >> "$dir/fake-adb.log"
env > "$dir/fake-adb.env"
[ "$1" = "-s" ] && shift 2
case "$*" in
  "devices -l")
    printf 'List of devices attached\nemulator-5554          device product:sdk_gphone64_arm64 model:Fake_Pixel transport_id:1\n' ;;
  "emu avd name") printf 'Fake_Pixel\r\nOK\r\n' ;;
  "shell getprop sys.boot_completed") echo 1 ;;
  "shell getprop ro.build.version.release") echo 16 ;;
  "exec-out screencap -p")
    if [ -f "$dir/fake-adb.flip" ]; then
      printf '%s' 'iVBORw0KGgoAAAANSUhEUgAAAGwAAADwCAIAAABqoBaiAAABt0lEQVR4nO3QQQkAIADAQOOYyYimNIQPhxwswLgx99Jl4/nBB0GE2AgixEYQITaCCLERRIiNIEJsBBFiI4gQG0GE2AgixEYQITaCCLERRIiNIEJsBBFiI4gQG0GE2AgixEYQITaCCLERRIiNIEJsBBFiI4gQG0GE2AgixEYQITaCCLERRIiNIEJsBBFiI4gQG0GE2AgixEYQITaCCLERRIiNIEJsBBFiI4gQG0GE2AgixEYQITaCCLERRIiNIEJsBBFiI4gQG0GE2AgixEYQITaCCLERRIiNIEJsBBFiI4gQG0GE2AgixEYQITaCCLERRIiNIEJsBBFiI4gQG0GE2AgixEYQITaCCLERRIiNIEJsBBFiI4gQG0GE2AgixEYQITaCCLERRIiNIEJsBBFiI4gQG0GE2AgixEYQITaCCLERRIiNIEJsBBFiI4gQG0GE2AgixEYQITaCCLERRIiNIEJsBBFiI4gQG0GE2AgixEYQITaCCLERRIiNIEJsBBFiI4gQG0GE2AgixEYQITaCCLERRIiNIEJsBBFiI4gQG0GE2AgixEYQITaCCLERRIiNIEJsBBFiowMyTq3rBigMqwAAAABJRU5ErkJggg==' | base64 --decode
    else
      printf '%s' 'iVBORw0KGgoAAAANSUhEUgAAAGwAAADwCAIAAABqoBaiAAABt0lEQVR4nO3QQQkAIADAQOOYycTGMoQPhxwswLgx19Zl4/nBB0GE2AgixEYQITaCCLERRIiNIEJsBBFiI4gQG0GE2AgixEYQITaCCLERRIiNIEJsBBFiI4gQG0GE2AgixEYQITaCCLERRIiNIEJsBBFiI4gQG0GE2AgixEYQITaCCLERRIiNIEJsBBFiI4gQG0GE2AgixEYQITaCCLERRIiNIEJsBBFiI4gQG0GE2AgixEYQITaCCLERRIiNIEJsBBFiI4gQG0GE2AgixEYQITaCCLERRIiNIEJsBBFiI4gQG0GE2AgixEYQITaCCLERRIiNIEJsBBFiI4gQG0GE2AgixEYQITaCCLERRIiNIEJsBBFiI4gQG0GE2AgixEYQITaCCLERRIiNIEJsBBFiI4gQG0GE2AgixEYQITaCCLERRIiNIEJsBBFiI4gQG0GE2AgixEYQITaCCLERRIiNIEJsBBFiI4gQG0GE2AgixEYQITaCCLERRIiNIEJsBBFiI4gQG0GE2AgixEYQITaCCLERRIiNIEJsBBFiI4gQG0GE2AgixEYQITaCCLERRIiNIEJsBBFiowPCWIwfXgIIXQAAAABJRU5ErkJggg==' | base64 --decode
    fi ;;
  "shell input "*)
    if [ -f "$dir/fake-adb.flip" ]; then rm "$dir/fake-adb.flip"; else : > "$dir/fake-adb.flip"; fi ;;
  *) echo "fake-adb: unknown command: $*" >&2; exit 1 ;;
esac
