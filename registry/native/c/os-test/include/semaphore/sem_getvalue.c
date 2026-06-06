#include <semaphore.h>
#ifdef sem_getvalue
#undef sem_getvalue
#endif
int (*foo)(sem_t *restrict, int *restrict) = sem_getvalue;
int main(void) { return 0; }
