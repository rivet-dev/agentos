/*[RPP|TPP]*/
#include <pthread.h>
#ifdef pthread_mutex_setprioceiling
#undef pthread_mutex_setprioceiling
#endif
int (*foo)(pthread_mutex_t *restrict, int, int *restrict) = pthread_mutex_setprioceiling;
int main(void) { return 0; }
