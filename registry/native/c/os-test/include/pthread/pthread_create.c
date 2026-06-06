#include <pthread.h>
#ifdef pthread_create
#undef pthread_create
#endif
int (*foo)(pthread_t *restrict, const pthread_attr_t *restrict, void *(*)(void*), void *restrict) = pthread_create;
int main(void) { return 0; }
