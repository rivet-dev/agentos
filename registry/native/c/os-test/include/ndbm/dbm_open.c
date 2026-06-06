/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <ndbm.h>
#ifdef dbm_open
#undef dbm_open
#endif
DBM *(*foo)(const char *, int, mode_t) = dbm_open;
int main(void) { return 0; }
