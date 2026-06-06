#include <pthread.h>
#ifdef pthread_cancel
#undef pthread_cancel
#endif
int (*foo)(pthread_t) = pthread_cancel;
int main(void) { return 0; }
