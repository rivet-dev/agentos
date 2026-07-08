/*
 * wget.c - minimal wget implementation built on libcurl
 *
 * Supports common wget options for HTTP/HTTPS downloads:
 *   URL                Download file to current directory (basename from URL)
 *   -O FILE            Write output to specific file ("-" for stdout)
 *   -q                 Quiet mode (suppress progress/messages)
 *   -L                 Follow redirects (enabled by default, like real wget)
 *   --no-check-certificate  Skip TLS certificate verification
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <curl/curl.h>

/* Write callback: write to FILE* */
static size_t write_callback(char *ptr, size_t size, size_t nmemb,
                             void *userdata) {
    FILE *out = (FILE *)userdata;
    return fwrite(ptr, size, nmemb, out);
}

/* Extract filename from URL path, fallback to "index.html" */
static const char *basename_from_url(const char *url) {
    /* Skip scheme */
    const char *p = strstr(url, "://");
    if (p) p += 3; else p = url;

    /* Find last '/' in path (before query string) */
    const char *last_slash = NULL;
    const char *q = p;
    while (*q && *q != '?' && *q != '#') {
        if (*q == '/') last_slash = q;
        q++;
    }

    if (last_slash && last_slash[1] && last_slash[1] != '?' && last_slash[1] != '#') {
        /* Extract filename between last slash and query/end */
        const char *start = last_slash + 1;
        size_t len = 0;
        while (start[len] && start[len] != '?' && start[len] != '#') len++;
        if (len > 0) {
            static char buf[256];
            if (len >= sizeof(buf)) len = sizeof(buf) - 1;
            memcpy(buf, start, len);
            buf[len] = '\0';
            return buf;
        }
    }

    return "index.html";
}

int main(int argc, char *argv[]) {
    const char *url = NULL;
    const char *output_file = NULL;
    int quiet = 0;
    int no_check_cert = 0;
    int output_to_stdout = 0;

    /* Parse arguments */
    for (int i = 1; i < argc; i++) {
        if (strcmp(argv[i], "-O") == 0 && i + 1 < argc) {
            output_file = argv[++i];
            if (strcmp(output_file, "-") == 0) {
                output_to_stdout = 1;
                output_file = NULL;
            }
        } else if (strcmp(argv[i], "-q") == 0) {
            quiet = 1;
        } else if (strcmp(argv[i], "-L") == 0) {
            /* Follow redirects — already default, accept silently */
        } else if (strcmp(argv[i], "--no-check-certificate") == 0) {
            no_check_cert = 1;
        } else if (argv[i][0] != '-') {
            url = argv[i];
        } else {
            /* Unknown option — skip silently for forward compat */
        }
    }

    if (!url) {
        fprintf(stderr, "wget: missing URL\nUsage: wget [OPTION]... [URL]...\n");
        return 1;
    }

    CURLcode res;
    curl_global_init(CURL_GLOBAL_DEFAULT);

    CURL *curl = curl_easy_init();
    if (!curl) {
        fprintf(stderr, "wget: failed to initialize\n");
        curl_global_cleanup();
        return 1;
    }

    /* Determine output destination */
    FILE *out = NULL;
    const char *dest_name = NULL;

    if (output_to_stdout) {
        out = stdout;
    } else {
        dest_name = output_file ? output_file : basename_from_url(url);
        out = fopen(dest_name, "wb");
        if (!out) {
            fprintf(stderr, "wget: cannot open '%s' for writing\n", dest_name);
            curl_easy_cleanup(curl);
            curl_global_cleanup();
            return 1;
        }
    }

    curl_easy_setopt(curl, CURLOPT_URL, url);
    curl_easy_setopt(curl, CURLOPT_WRITEFUNCTION, write_callback);
    curl_easy_setopt(curl, CURLOPT_WRITEDATA, out);

    /* wget follows redirects by default */
    curl_easy_setopt(curl, CURLOPT_FOLLOWLOCATION, 1L);
    curl_easy_setopt(curl, CURLOPT_MAXREDIRS, 20L);

    /* Suppress progress meter */
    curl_easy_setopt(curl, CURLOPT_NOPROGRESS, 1L);

    /* TLS: skip certificate verification */
    if (no_check_cert) {
        curl_easy_setopt(curl, CURLOPT_SSL_VERIFYPEER, 0L);
        curl_easy_setopt(curl, CURLOPT_SSL_VERIFYHOST, 0L);
    }

    /* Perform request */
    res = curl_easy_perform(curl);

    int exit_code = 0;

    if (res != CURLE_OK) {
        if (!quiet) {
            fprintf(stderr, "wget: failed: %s\n", curl_easy_strerror(res));
        }
        exit_code = 1;
        /* Remove partial download on failure (unless stdout) */
        if (!output_to_stdout && dest_name) {
            fclose(out);
            out = NULL;
            remove(dest_name);
        }
    } else {
        /* Check HTTP response code */
        long http_code = 0;
        curl_easy_getinfo(curl, CURLINFO_RESPONSE_CODE, &http_code);
        if (http_code >= 400) {
            if (!quiet) {
                fprintf(stderr, "wget: server returned HTTP %ld\n", http_code);
            }
            exit_code = 8; /* wget uses exit code 8 for server errors */
            /* Remove error response file (unless stdout) */
            if (!output_to_stdout && dest_name) {
                fclose(out);
                out = NULL;
                remove(dest_name);
            }
        }
    }

    /* Cleanup */
    curl_easy_cleanup(curl);
    if (out && out != stdout) {
        fclose(out);
    }
    curl_global_cleanup();

    return exit_code;
}
