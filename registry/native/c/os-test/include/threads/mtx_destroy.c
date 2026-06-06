#include <threads.h>
#ifdef mtx_destroy
#undef mtx_destroy
#endif
void (*foo)(mtx_t *) = mtx_destroy;
int main(void) { return 0; }
