#include <pthread.h>
#ifdef pthread_attr_init
#undef pthread_attr_init
#endif
int (*foo)(pthread_attr_t *) = pthread_attr_init;
int main(void) { return 0; }
