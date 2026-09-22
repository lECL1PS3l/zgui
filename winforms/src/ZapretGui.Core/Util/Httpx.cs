using System;
using System.Net.Http;
using System.Net.Http.Headers;
using System.Threading.Tasks;

namespace ZapretGui.Core.Util
{
    /// <summary>
    /// HTTP-клиент обновлений — порт updater.rs:17-36: UA, таймаут 45 c, редиректы.
    /// Один клиент на процесс (reqwest::Client переиспользуется).
    /// </summary>
    public static class Httpx
    {
        public const string UA = "zgui/0.1 (zapret-gui updater)";

        private const string GitHubAccept = "application/vnd.github+json";

        private static readonly TimeSpan Timeout45s = TimeSpan.FromSeconds(45);

        private static HttpClient _shared;

        public static HttpClient Client()
        {
            if (_shared == null)
            {
                _shared = new HttpClient { Timeout = Timeout45s };
                _shared.DefaultRequestHeaders.UserAgent.ParseAdd(UA);
            }
            return _shared;
        }

        /// <summary>Байты URL с Cache-Control: no-cache (updater.rs:190-202).</summary>
        public static async Task<Result<byte[]>> FetchBytesAsync(HttpClient cli, string url)
        {
            try
            {
                using (HttpRequestMessage req = new HttpRequestMessage(HttpMethod.Get, url))
                {
                    req.Headers.CacheControl = new CacheControlHeaderValue { NoCache = true };
                    using (HttpResponseMessage resp = await cli.SendAsync(req).ConfigureAwait(false))
                    {
                        if (!resp.IsSuccessStatusCode)
                        {
                            return Result<byte[]>.Err(url + ": HTTP " + (int)resp.StatusCode);
                        }
                        return Result<byte[]>.Ok(await resp.Content.ReadAsByteArrayAsync().ConfigureAwait(false));
                    }
                }
            }
            catch (Exception e)
            {
                return Result<byte[]>.Err(url + ": " + e.Message);
            }
        }

        /// <summary>JSON GitHub API с Accept: application/vnd.github+json.</summary>
        public static async Task<Result<string>> GetJsonAsync(HttpClient cli, string url)
        {
            try
            {
                using (HttpRequestMessage req = new HttpRequestMessage(HttpMethod.Get, url))
                {
                    req.Headers.Accept.Add(new MediaTypeWithQualityHeaderValue(GitHubAccept));
                    using (HttpResponseMessage resp = await cli.SendAsync(req).ConfigureAwait(false))
                    {
                        if (!resp.IsSuccessStatusCode)
                        {
                            return Result<string>.Err("GitHub API: HTTP " + (int)resp.StatusCode);
                        }
                        return Result<string>.Ok(await resp.Content.ReadAsStringAsync().ConfigureAwait(false));
                    }
                }
            }
            catch (Exception e)
            {
                return Result<string>.Err(e.Message);
            }
        }
    }
}
