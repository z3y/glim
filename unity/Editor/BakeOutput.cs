using System;
using System.Collections.Generic;
using System.IO;
using UnityEditor;
using UnityEngine;

namespace Glim
{
    internal static class BakeOutput
    {
        const string BackupFolder = "previous";

        sealed class PublishedFile
        {
            public string destination;
            public string backup;
            public FileStream stream;
        }

        internal static bool Publish(string stagingDirectory, string outputDirectory)
        {
            var files = Directory.GetFiles(stagingDirectory);
            Array.Sort(files, StringComparer.Ordinal);
            var backupDirectory = Path.Combine(stagingDirectory, BackupFolder);
            Directory.CreateDirectory(backupDirectory);
            var published = new List<PublishedFile>();
            bool complete = false;

            // A lock dialog can pump editor updates while some outputs still need to be written.
            AssetDatabase.DisallowAutoRefresh();
            try
            {
                foreach (var source in files)
                {
                    var name = Path.GetFileName(source);
                    var destination = Path.Combine(outputDirectory, name);
                    if (name.EndsWith(".meta", StringComparison.Ordinal) && File.Exists(destination))
                    {
                        continue;
                    }

                    using var input = File.OpenRead(source);
                    PublishedFile file;
                    while (true)
                    {
                        if (Bake.IsCancelling)
                        {
                            return false;
                        }
                        try
                        {
                            file = OpenForWriting(destination, Path.Combine(backupDirectory, name));
                            break;
                        }
                        catch (IOException error) when (IsWindowsLock(error))
                        {
                            var message = $"Another process is using {destination}.";
                            if (Application.isBatchMode)
                            {
                                throw new IOException(message, error);
                            }

                            if (!EditorUtility.DisplayDialog("Lightmap file in use",
                                message + "\n\nWait for the file to become available, then retry. Cancel keeps the previous bake.",
                                "Retry", "Cancel Bake"))
                            {
                                Debug.LogWarning($"Glim: Bake canceled while saving {destination}.");
                                return false;
                            }
                        }
                    }
                    // Keep the handle until the whole set is saved so rollback can restore this file.
                    published.Add(file);
                    input.CopyTo(file.stream);
                    file.stream.Flush();
                }

                complete = true;
                return true;
            }
            finally
            {
                try
                {
                    CloseFiles(published, backupDirectory, complete);
                }
                finally
                {
                    AssetDatabase.AllowAutoRefresh();
                }
            }
        }

        static bool IsWindowsLock(IOException error)
        {
            return Application.platform == RuntimePlatform.WindowsEditor &&
                (error.HResult == unchecked((int)0x80070020) || // ERROR_SHARING_VIOLATION
                 error.HResult == unchecked((int)0x80070021) || // ERROR_LOCK_VIOLATION
                 error.HResult == unchecked((int)0x800704C8)); // ERROR_USER_MAPPED_FILE
        }

        static PublishedFile OpenForWriting(string destination, string backup)
        {
            bool exists = File.Exists(destination);
            FileStream output = new(destination, exists ? FileMode.Open : FileMode.CreateNew,
                FileAccess.ReadWrite, FileShare.None);
            try
            {
                if (exists)
                {
                    using (var previous = File.Create(backup))
                    {
                        output.CopyTo(previous);
                    }
                    output.Position = 0;
                    // A mapped file can still open for writing after its original handle closes.
                    // Windows rejects truncation without changing the old file in that case.
                    output.SetLength(0);
                }

                return new PublishedFile { destination = destination, backup = exists ? backup : null, stream = output };
            }
            catch
            {
                output.Dispose();
                throw;
            }
        }

        static void CloseFiles(List<PublishedFile> published, string backupDirectory, bool complete)
        {
            bool keepBackups = false;
            for (int i = published.Count - 1; i >= 0; i--)
            {
                var file = published[i];
                try
                {
                    using (file.stream)
                    {
                        if (!complete && file.backup != null)
                        {
                            using var previous = File.OpenRead(file.backup);
                            file.stream.Position = 0;
                            previous.CopyTo(file.stream);
                            file.stream.SetLength(previous.Length);
                        }
                    }
                    if (!complete && file.backup == null)
                    {
                        File.Delete(file.destination);
                    }
                }
                catch (Exception error)
                {
                    keepBackups = true;
                    Debug.LogError($"Glim could not finish saving {file.destination}. Backup folder: {backupDirectory}\n{error}");
                }
            }

            if (!keepBackups)
            {
                try
                {
                    Directory.Delete(backupDirectory, true);
                }
                catch (Exception error) when (error is IOException || error is UnauthorizedAccessException)
                {
                    Debug.LogWarning($"Glim could not remove temporary bake backups at {backupDirectory}: {error.Message}");
                }
            }
        }

        internal static void Discard(string stagingDirectory)
        {
            // Keep prior results available for manual recovery if restoring a file failed.
            if (stagingDirectory == null || !Directory.Exists(stagingDirectory) ||
                Directory.Exists(Path.Combine(stagingDirectory, BackupFolder)))
            {
                return;
            }

            Directory.Delete(stagingDirectory, true);
        }
    }
}
