/*[TPS]*/
#include <pthread.h>
#ifdef pthread_attr_getscope
#undef pthread_attr_getscope
#endif
int (*foo)(const pthread_attr_t *restrict, int *restrict) = pthread_attr_getscope;
int main(void) { return 0; }
