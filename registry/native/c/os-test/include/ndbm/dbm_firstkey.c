/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <ndbm.h>
#ifdef dbm_firstkey
#undef dbm_firstkey
#endif
datum (*foo)(DBM *) = dbm_firstkey;
int main(void) { return 0; }
