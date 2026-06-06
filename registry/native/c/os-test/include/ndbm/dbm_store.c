/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <ndbm.h>
#ifdef dbm_store
#undef dbm_store
#endif
int (*foo)(DBM *, datum, datum, int) = dbm_store;
int main(void) { return 0; }
