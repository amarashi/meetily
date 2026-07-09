"use client"

import { useEffect, useState, useRef } from "react"
import { Switch } from "./ui/switch"
import { FolderOpen, Plus, Trash2 } from "lucide-react"
import { invoke } from "@tauri-apps/api/core"
import Analytics from "@/lib/analytics"
import AnalyticsConsentSwitch from "./AnalyticsConsentSwitch"
import { useConfig, NotificationSettings } from "@/contexts/ConfigContext"

interface DictationSettings {
  cleanup_enabled: boolean;
  cleanup_model: string;
  ollama_endpoint: string;
}

interface DictionaryEntry {
  id: string;
  misheard: string | null;
  correct: string;
}

export function PreferenceSettings() {
  const {
    notificationSettings,
    storageLocations,
    isLoadingPreferences,
    loadPreferences,
    updateNotificationSettings
  } = useConfig();

  const [notificationsEnabled, setNotificationsEnabled] = useState<boolean | null>(null);
  const [dictationSettings, setDictationSettings] = useState<DictationSettings | null>(null);
  const [dictionaryEntries, setDictionaryEntries] = useState<DictionaryEntry[]>([]);
  const [newMisheard, setNewMisheard] = useState('');
  const [newCorrect, setNewCorrect] = useState('');
  const [isInitialLoad, setIsInitialLoad] = useState(true);
  const [previousNotificationsEnabled, setPreviousNotificationsEnabled] = useState<boolean | null>(null);
  const hasTrackedViewRef = useRef(false);

  // Lazy load preferences on mount (only loads if not already cached)
  useEffect(() => {
    loadPreferences();
    // Reset tracking ref on mount (every tab visit)
    hasTrackedViewRef.current = false;
  }, [loadPreferences]);

  // Load dictation settings on mount
  useEffect(() => {
    invoke<DictationSettings>('get_dictation_settings')
      .then(setDictationSettings)
      .catch((error) => console.error('Failed to load dictation settings:', error));
  }, []);

  const updateDictationSettings = async (updated: DictationSettings) => {
    setDictationSettings(updated);
    try {
      await invoke('set_dictation_settings', { settings: updated });
    } catch (error) {
      console.error('Failed to save dictation settings:', error);
    }
  };

  // Load dictionary entries on mount
  useEffect(() => {
    invoke<DictionaryEntry[]>('get_dictionary_entries')
      .then(setDictionaryEntries)
      .catch((error) => console.error('Failed to load dictionary:', error));
  }, []);

  const handleAddDictionaryEntry = async () => {
    const correct = newCorrect.trim();
    if (!correct) return;
    const misheard = newMisheard.trim() || null;
    try {
      const entry = await invoke<DictionaryEntry>('add_dictionary_entry', { misheard, correct });
      setDictionaryEntries((prev) =>
        prev.some((e) => e.id === entry.id) ? prev : [...prev, entry]
      );
      setNewMisheard('');
      setNewCorrect('');
    } catch (error) {
      console.error('Failed to add dictionary entry:', error);
    }
  };

  const handleDeleteDictionaryEntry = async (id: string) => {
    try {
      await invoke('delete_dictionary_entry', { id });
      setDictionaryEntries((prev) => prev.filter((e) => e.id !== id));
    } catch (error) {
      console.error('Failed to delete dictionary entry:', error);
    }
  };

  // Track preferences viewed analytics on every tab visit (once per mount)
  useEffect(() => {
    if (hasTrackedViewRef.current) return;

    const trackPreferencesViewed = async () => {
      // Wait for notification settings to be available (either from cache or after loading)
      if (notificationSettings) {
        await Analytics.track('preferences_viewed', {
          notifications_enabled: notificationSettings.notification_preferences.show_recording_started ? 'true' : 'false'
        });
        hasTrackedViewRef.current = true;
      } else if (!isLoadingPreferences) {
        // If not loading and no settings available, track with default value
        await Analytics.track('preferences_viewed', {
          notifications_enabled: 'false'
        });
        hasTrackedViewRef.current = true;
      }
    };

    trackPreferencesViewed();
  }, [notificationSettings, isLoadingPreferences]);

  // Update notificationsEnabled when notificationSettings are loaded from global state
  useEffect(() => {
    if (notificationSettings) {
      // Notification enabled means both started and stopped notifications are enabled
      const enabled =
        notificationSettings.notification_preferences.show_recording_started &&
        notificationSettings.notification_preferences.show_recording_stopped;
      setNotificationsEnabled(enabled);
      if (isInitialLoad) {
        setPreviousNotificationsEnabled(enabled);
        setIsInitialLoad(false);
      }
    } else if (!isLoadingPreferences) {
      // If not loading and no settings, use default
      setNotificationsEnabled(true);
      if (isInitialLoad) {
        setPreviousNotificationsEnabled(true);
        setIsInitialLoad(false);
      }
    }
  }, [notificationSettings, isLoadingPreferences, isInitialLoad])

  useEffect(() => {
    // Skip update on initial load or if value hasn't actually changed
    if (isInitialLoad || notificationsEnabled === null || notificationsEnabled === previousNotificationsEnabled) return;
    if (!notificationSettings) return;

    const handleUpdateNotificationSettings = async () => {
      console.log("Updating notification settings to:", notificationsEnabled);

      try {
        // Update the notification preferences
        const updatedSettings: NotificationSettings = {
          ...notificationSettings,
          notification_preferences: {
            ...notificationSettings.notification_preferences,
            show_recording_started: notificationsEnabled,
            show_recording_stopped: notificationsEnabled,
          }
        };

        console.log("Calling updateNotificationSettings with:", updatedSettings);
        await updateNotificationSettings(updatedSettings);
        setPreviousNotificationsEnabled(notificationsEnabled);
        console.log("Successfully updated notification settings to:", notificationsEnabled);

        // Track notification preference change - only fires when user manually toggles
        await Analytics.track('notification_settings_changed', {
          notifications_enabled: notificationsEnabled.toString()
        });
      } catch (error) {
        console.error('Failed to update notification settings:', error);
      }
    };

    handleUpdateNotificationSettings();
  }, [notificationsEnabled, notificationSettings, isInitialLoad, previousNotificationsEnabled, updateNotificationSettings])

  const handleOpenFolder = async (folderType: 'database' | 'models' | 'recordings') => {
    try {
      switch (folderType) {
        case 'database':
          await invoke('open_database_folder');
          break;
        case 'models':
          await invoke('open_models_folder');
          break;
        case 'recordings':
          await invoke('open_recordings_folder');
          break;
      }

      // Track storage folder access
      await Analytics.track('storage_folder_opened', {
        folder_type: folderType
      });
    } catch (error) {
      console.error(`Failed to open ${folderType} folder:`, error);
    }
  };

  // Show loading only if we're actually loading and don't have cached data
  if (isLoadingPreferences && !notificationSettings && !storageLocations) {
    return <div className="max-w-2xl mx-auto p-6">Loading Preferences...</div>
  }

  // Show loading if notificationsEnabled hasn't been determined yet
  if (notificationsEnabled === null && !isLoadingPreferences) {
    return <div className="max-w-2xl mx-auto p-6">Loading Preferences...</div>
  }

  // Ensure we have a boolean value for the Switch component
  const notificationsEnabledValue = notificationsEnabled ?? false;

  return (
    <div className="space-y-6">
      {/* Notifications Section */}
      <div className="bg-white rounded-lg border border-gray-200 p-6 shadow-sm">
        <div className="flex items-center justify-between">
          <div>
            <h3 className="text-lg font-semibold text-gray-900 mb-2">Notifications</h3>
            <p className="text-sm text-gray-600">Enable or disable notifications of start and end of meeting</p>
          </div>
          <Switch checked={notificationsEnabledValue} onCheckedChange={setNotificationsEnabled} />
        </div>
      </div>

      {/* Dictation Section */}
      <div className="bg-white rounded-lg border border-gray-200 p-6 shadow-sm">
        <h3 className="text-lg font-semibold text-gray-900 mb-2">Dictation (Win+Shift+Z)</h3>
        <p className="text-sm text-gray-600 mb-4">
          System-wide voice typing into the focused window
        </p>

        <div className="space-y-4">
          <div className="flex items-center justify-between">
            <div>
              <p className="text-sm font-medium text-gray-700">Clean up text with AI before typing</p>
              <p className="text-xs text-gray-500">
                Removes filler words, stutters, and false starts using a local Ollama model.
                Falls back to raw text if Ollama is unavailable.
              </p>
            </div>
            <Switch
              checked={dictationSettings?.cleanup_enabled ?? true}
              onCheckedChange={(checked) => {
                if (dictationSettings) {
                  updateDictationSettings({ ...dictationSettings, cleanup_enabled: checked });
                }
              }}
              disabled={!dictationSettings}
            />
          </div>

          {dictationSettings?.cleanup_enabled && (
            <div>
              <label className="block text-sm font-medium text-gray-700 mb-1">
                Cleanup model
              </label>
              <input
                type="text"
                value={dictationSettings.cleanup_model}
                onChange={(e) =>
                  setDictationSettings({ ...dictationSettings, cleanup_model: e.target.value })
                }
                onBlur={() => updateDictationSettings(dictationSettings)}
                placeholder="gemma3:4b"
                className="w-full px-3 py-2 text-sm border border-gray-300 rounded-md shadow-sm focus:outline-none focus:ring-1 focus:ring-blue-500 focus:border-blue-500"
              />
              <p className="text-xs text-gray-500 mt-1">
                Ollama model name. The default <span className="font-mono">gemma3:4b</span> is fast and
                handles both English and Persian well (install with{' '}
                <span className="font-mono">ollama pull gemma3:4b</span>).
              </p>
            </div>
          )}
        </div>
      </div>

      {/* Dictionary Section */}
      <div className="bg-white rounded-lg border border-gray-200 p-6 shadow-sm">
        <h3 className="text-lg font-semibold text-gray-900 mb-2">Dictionary</h3>
        <p className="text-sm text-gray-600 mb-4">
          Names, companies, medications, or words you pronounce differently. Used to improve
          transcription in meetings and dictation. Fixing a transcript in a meeting adds entries
          here automatically.
        </p>

        {/* Add entry form */}
        <div className="flex gap-2 mb-4">
          <input
            type="text"
            dir="auto"
            value={newMisheard}
            onChange={(e) => setNewMisheard(e.target.value)}
            placeholder="Misheard as (optional)"
            className="flex-1 px-3 py-2 text-sm border border-gray-300 rounded-md shadow-sm focus:outline-none focus:ring-1 focus:ring-blue-500 focus:border-blue-500"
          />
          <input
            type="text"
            dir="auto"
            value={newCorrect}
            onChange={(e) => setNewCorrect(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && handleAddDictionaryEntry()}
            placeholder="Correct word or phrase"
            className="flex-1 px-3 py-2 text-sm border border-gray-300 rounded-md shadow-sm focus:outline-none focus:ring-1 focus:ring-blue-500 focus:border-blue-500"
          />
          <button
            onClick={handleAddDictionaryEntry}
            disabled={!newCorrect.trim()}
            className="flex items-center gap-1 px-3 py-2 text-sm font-medium text-white bg-blue-600 rounded-md hover:bg-blue-700 disabled:opacity-50 disabled:cursor-not-allowed"
          >
            <Plus className="w-4 h-4" />
            Add
          </button>
        </div>

        {/* Entries list */}
        {dictionaryEntries.length === 0 ? (
          <p className="text-sm text-gray-400">
            No entries yet. Add a word above, or fix a transcript in any meeting.
          </p>
        ) : (
          <div className="max-h-64 overflow-y-auto divide-y divide-gray-100 border border-gray-100 rounded-md">
            {dictionaryEntries.map((entry) => (
              <div key={entry.id} className="flex items-center gap-2 px-3 py-2 text-sm">
                <span dir="auto" className="flex-1 text-gray-500">
                  {entry.misheard ?? <em className="text-gray-400">any similar sound</em>}
                </span>
                <span className="text-gray-400">→</span>
                <span dir="auto" className="flex-1 font-medium text-gray-800">{entry.correct}</span>
                <button
                  onClick={() => handleDeleteDictionaryEntry(entry.id)}
                  title="Remove entry"
                  className="p-1 text-gray-400 hover:text-red-600 hover:bg-red-50 rounded"
                >
                  <Trash2 className="w-4 h-4" />
                </button>
              </div>
            ))}
          </div>
        )}
      </div>

      {/* Data Storage Locations Section */}
      <div className="bg-white rounded-lg border border-gray-200 p-6 shadow-sm">
        <h3 className="text-lg font-semibold text-gray-900 mb-4">Data Storage Locations</h3>
        <p className="text-sm text-gray-600 mb-6">
          View and access where Meetily stores your data
        </p>

        <div className="space-y-4">
          {/* Database Location */}
          {/* <div className="p-4 border rounded-lg bg-gray-50">
            <div className="font-medium mb-2">Database</div>
            <div className="text-sm text-gray-600 mb-3 break-all font-mono text-xs">
              {storageLocations?.database || 'Loading...'}
            </div>
            <button
              onClick={() => handleOpenFolder('database')}
              className="flex items-center gap-2 px-3 py-2 text-sm border border-gray-300 rounded-md hover:bg-gray-100 transition-colors"
            >
              <FolderOpen className="w-4 h-4" />
              Open Folder
            </button>
          </div> */}

          {/* Models Location */}
          {/* <div className="p-4 border rounded-lg bg-gray-50">
            <div className="font-medium mb-2">Whisper Models</div>
            <div className="text-sm text-gray-600 mb-3 break-all font-mono text-xs">
              {storageLocations?.models || 'Loading...'}
            </div>
            <button
              onClick={() => handleOpenFolder('models')}
              className="flex items-center gap-2 px-3 py-2 text-sm border border-gray-300 rounded-md hover:bg-gray-100 transition-colors"
            >
              <FolderOpen className="w-4 h-4" />
              Open Folder
            </button>
          </div> */}

          {/* Recordings Location */}
          <div className="p-4 border rounded-lg bg-gray-50">
            <div className="font-medium mb-2">Meeting Recordings</div>
            <div className="text-sm text-gray-600 mb-3 break-all font-mono text-xs">
              {storageLocations?.recordings || 'Loading...'}
            </div>
            <button
              onClick={() => handleOpenFolder('recordings')}
              className="flex items-center gap-2 px-3 py-2 text-sm border border-gray-300 rounded-md hover:bg-gray-100 transition-colors"
            >
              <FolderOpen className="w-4 h-4" />
              Open Folder
            </button>
          </div>
        </div>

        <div className="mt-4 p-3 bg-blue-50 rounded-md">
          <p className="text-xs text-blue-800">
            <strong>Note:</strong> Database and models are stored together in your application data directory for unified management.
          </p>
        </div>
      </div>

      {/* Analytics Section */}
      <div className="bg-white rounded-lg border border-gray-200 p-6 shadow-sm">
        <AnalyticsConsentSwitch />
      </div>
    </div>
  )
}
