#include <pthread.h>
#ifdef pthread_key_delete
#undef pthread_key_delete
#endif
int (*foo)(pthread_key_t) = pthread_key_delete;
int main(void) { return 0; }
