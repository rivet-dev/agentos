#include <pthread.h>
#ifdef pthread_getspecific
#undef pthread_getspecific
#endif
void *(*foo)(pthread_key_t) = pthread_getspecific;
int main(void) { return 0; }
