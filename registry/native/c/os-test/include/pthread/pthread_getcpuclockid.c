/*[TCT]*/
#include <pthread.h>
#ifdef pthread_getcpuclockid
#undef pthread_getcpuclockid
#endif
int (*foo)(pthread_t, clockid_t *) = pthread_getcpuclockid;
int main(void) { return 0; }
