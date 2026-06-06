/*[TPS]*/
#include <pthread.h>
#ifdef pthread_attr_getinheritsched
#undef pthread_attr_getinheritsched
#endif
int (*foo)(const pthread_attr_t *restrict, int *restrict) = pthread_attr_getinheritsched;
int main(void) { return 0; }
