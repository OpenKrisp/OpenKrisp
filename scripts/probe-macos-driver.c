// Load built HAL plugins in this process only. Never install or contact the audio server.
#include <CoreAudio/AudioServerPlugIn.h>
#include <CoreFoundation/CoreFoundation.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define CHECK(condition) do { if (!(condition)) { \
    fprintf(stderr, "Driver property check failed at line %d\n", __LINE__); return 1; \
} } while (0)

static int string_property(AudioServerPlugInDriverRef driver, AudioObjectID object,
                           AudioObjectPropertySelector selector, const char *expected) {
    AudioObjectPropertyAddress address = {selector, kAudioObjectPropertyScopeGlobal, 0};
    CFStringRef actual = NULL;
    UInt32 size = 0;
    CHECK((*driver)->GetPropertyData(driver, object, 0, &address, 0, NULL,
                                     sizeof(actual), &size, &actual) == 0);
    CHECK(size == sizeof(actual) && actual != NULL);
    CFStringRef value = CFStringCreateWithCString(NULL, expected, kCFStringEncodingUTF8);
    int equal = CFEqual(actual, value);
    CFRelease(actual);
    CFRelease(value);
    CHECK(equal);
    return 0;
}

int main(int argc, char **argv) {
    CHECK(argc >= 5 && (argc - 1) % 4 == 0);
    // Keep all plugins loaded together to catch conflicting CFPlugIn factory identities.
    for (int i = 1; i < argc; i += 4) {
        CFURLRef url = CFURLCreateFromFileSystemRepresentation(NULL,
            (const UInt8 *)argv[i], strlen(argv[i]), true);
        CFPlugInRef plugin = CFPlugInCreate(NULL, url);
        CFRelease(url);
        CHECK(plugin != NULL);
        CFArrayRef factories = CFPlugInFindFactoriesForPlugInTypeInPlugIn(
            kAudioServerPlugInTypeUUID, plugin);
        CHECK(factories != NULL && CFArrayGetCount(factories) == 1);
        AudioServerPlugInDriverRef driver = CFPlugInInstanceCreate(NULL,
            CFArrayGetValueAtIndex(factories, 0), kAudioServerPlugInTypeUUID);
        CHECK(driver != NULL);
        CHECK(string_property(driver, 3, kAudioObjectPropertyName, argv[i + 1]) == 0);
        CHECK(string_property(driver, 3, kAudioDevicePropertyDeviceUID, argv[i + 2]) == 0);
        char mirror[256];
        size_t uid_size = strlen(argv[i + 2]);
        CHECK(uid_size > 4 && uid_size < sizeof(mirror) - 3);
        snprintf(mirror, sizeof(mirror), "%.*s_2_UID", (int)uid_size - 4, argv[i + 2]);
        CHECK(string_property(driver, 12, kAudioDevicePropertyDeviceUID, mirror) == 0);
        AudioObjectPropertyAddress address = {kAudioStreamPropertyVirtualFormat,
            kAudioObjectPropertyScopeGlobal, 0};
        for (AudioObjectID stream = 4; stream <= 7; stream += 3) {
            AudioStreamBasicDescription format = {0};
            UInt32 size = 0;
            CHECK((*driver)->GetPropertyData(driver, stream, 0, &address, 0, NULL,
                sizeof(format), &size, &format) == 0);
            CHECK(size == sizeof(format));
            CHECK(format.mChannelsPerFrame == (UInt32)atoi(argv[i + 3]));
        }
        for (AudioObjectID object = 3; object <= 12; object += 9) {
            UInt32 value = 0, size = 0;
            address.mSelector = kAudioDevicePropertyTransportType;
            CHECK((*driver)->GetPropertyData(driver, object, 0, &address, 0, NULL,
                sizeof(value), &size, &value) == 0);
            CHECK(value == kAudioDeviceTransportTypeVirtual);
            address.mSelector = kAudioDevicePropertyIsHidden;
            CHECK((*driver)->GetPropertyData(driver, object, 0, &address, 0, NULL,
                sizeof(value), &size, &value) == 0);
            CHECK(value == (object == 12));
        }
        printf("Verified native driver: %s (%s, %s channels)\n",
            argv[i + 1], argv[i + 2], argv[i + 3]);
        CFRelease(factories);
        // Process exit releases plugin instances; never unload code behind a live interface.
    }
    return 0;
}
