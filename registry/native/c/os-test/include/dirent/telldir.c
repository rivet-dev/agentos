/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <dirent.h>
#ifdef telldir
#undef telldir
#endif
long (*foo)(DIR *) = telldir;
int main(void) { return 0; }
