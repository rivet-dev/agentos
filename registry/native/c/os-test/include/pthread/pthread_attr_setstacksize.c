/*[TSS]*/
#include <pthread.h>
#ifdef pthread_attr_setstacksize
#undef pthread_attr_setstacksize
#endif
int (*foo)(pthread_attr_t *, size_t) = pthread_attr_setstacksize;
int main(void) { return 0; }
