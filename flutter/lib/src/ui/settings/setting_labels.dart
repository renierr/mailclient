import '../../models/settings.dart';
import '../../sync/background_alarm.dart';

/// How a preference's value reads in the settings form. The values
/// themselves come from the core (`SettingChoices`); the wording matches the
/// Qt form's.
abstract final class SettingLabels {
  static String of(String key, Object value) => switch ((key, value)) {
    (SettingKeys.uiScale, final num v) => '${(v * 100).round()}%',
    (SettingKeys.readerFontSize, 'small') => 'Small',
    (SettingKeys.readerFontSize, 'large') => 'Large',
    (SettingKeys.readerFontSize, _) => 'Normal',
    (SettingKeys.sortField, 'from') => 'Sender',
    (SettingKeys.sortField, 'subject') => 'Subject',
    (SettingKeys.sortField, _) => 'Date',
    (SettingKeys.listDensity, 'compact') => 'Compact',
    (SettingKeys.listDensity, _) => 'Comfortable',
    (SettingKeys.markReadDelay, 0) => 'Immediately',
    (SettingKeys.markReadDelay, final int v) => 'After $v seconds',
    (SettingKeys.linkClickAction, 'browser') => 'Open directly in browser',
    (SettingKeys.linkClickAction, _) =>
      'Show safety dialog first (recommended)',
    (SettingKeys.sendFormat, 'plain') => 'Plain text (safest)',
    (SettingKeys.sendFormat, 'multipart') => 'Multipart plain + HTML',
    (SettingKeys.sendFormat, 'html') => 'HTML only',
    (SettingKeys.sendFormat, _) => 'Automatic (recommended)',
    (SettingKeys.syncInterval, 0) => 'Manually',
    (SettingKeys.syncInterval, 60) => 'Every hour',
    (SettingKeys.syncInterval, final int v) => 'Every $v minutes',
    (SettingKeys.backgroundScheduler, schedulerAlarm) => 'On-time alarm',
    (SettingKeys.backgroundScheduler, schedulerPush) => 'Push (IMAP IDLE)',
    (SettingKeys.backgroundScheduler, _) => 'Battery-saving (recommended)',
    _ => '$value',
  };
}
