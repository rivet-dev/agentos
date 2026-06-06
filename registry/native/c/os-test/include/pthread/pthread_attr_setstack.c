/*[TSA TSS]*/
#include <pthread.h>
#ifdef pthread_attr_setstack
#undef pthread_attr_setstack
#endif
int (*foo)(pthread_attr_t *, void *, size_t) = pthread_attr_setstack;
int main(void) { return 0; }
