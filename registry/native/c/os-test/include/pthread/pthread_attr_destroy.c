#include <pthread.h>
#ifdef pthread_attr_destroy
#undef pthread_attr_destroy
#endif
int (*foo)(pthread_attr_t *) = pthread_attr_destroy;
int main(void) { return 0; }
