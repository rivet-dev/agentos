/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <ftw.h>
#ifdef nftw
#undef nftw
#endif
int (*foo)(const char *, int (*)(const char *, const struct stat *, int, struct FTW *), int, int) = nftw;
int main(void) { return 0; }
