/*[RPP|TPP]*/
#include <pthread.h>
#ifdef pthread_mutex_getprioceiling
#undef pthread_mutex_getprioceiling
#endif
int (*foo)(const pthread_mutex_t *restrict, int *restrict) = pthread_mutex_getprioceiling;
int main(void) { return 0; }
