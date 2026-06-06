#include <semaphore.h>
#ifdef sem_open
#undef sem_open
#endif
sem_t *(*foo)(const char *, int, ...) = sem_open;
int main(void) { return 0; }
