#include <pthread.h>
#ifdef pthread_attr_getguardsize
#undef pthread_attr_getguardsize
#endif
int (*foo)(const pthread_attr_t *restrict, size_t *restrict) = pthread_attr_getguardsize;
int main(void) { return 0; }
