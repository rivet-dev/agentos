/*[TSA TSS]*/
#include <pthread.h>
#ifdef pthread_attr_getstack
#undef pthread_attr_getstack
#endif
int (*foo)(const pthread_attr_t *restrict, void **restrict, size_t *restrict) = pthread_attr_getstack;
int main(void) { return 0; }
