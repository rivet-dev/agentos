/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <ndbm.h>
#ifdef dbm_error
#undef dbm_error
#endif
int (*foo)(DBM *) = dbm_error;
int main(void) { return 0; }
