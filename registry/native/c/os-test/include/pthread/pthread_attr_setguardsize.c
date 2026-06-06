#include <pthread.h>
#ifdef pthread_attr_setguardsize
#undef pthread_attr_setguardsize
#endif
int (*foo)(pthread_attr_t *, size_t) = pthread_attr_setguardsize;
int main(void) { return 0; }
