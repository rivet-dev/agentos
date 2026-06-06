/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <dirent.h>
#ifdef seekdir
#undef seekdir
#endif
void (*foo)(DIR *, long) = seekdir;
int main(void) { return 0; }
