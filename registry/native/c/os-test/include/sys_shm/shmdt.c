/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <sys/shm.h>
#ifdef shmdt
#undef shmdt
#endif
int (*foo)(const void *) = shmdt;
int main(void) { return 0; }
