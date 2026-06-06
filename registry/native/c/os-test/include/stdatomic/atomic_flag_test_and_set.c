#include <stdatomic.h>
#ifdef atomic_flag_test_and_set
#undef atomic_flag_test_and_set
#endif
_Bool (*foo)(volatile atomic_flag *) = atomic_flag_test_and_set;
int main(void) { return 0; }
