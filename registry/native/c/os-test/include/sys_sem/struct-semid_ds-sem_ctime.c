/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <sys/sem.h>
void foo(struct semid_ds* bar)
{
	time_t *qux = &bar->sem_ctime;
	(void) qux;
}
int main(void) { return 0; }
