#include "internal/napi_ref_tracker.h"

void napi_ref_tracker__::Link(RefList* list) {
  prev_ = list;
  next_ = list->next_;
  if (next_ != nullptr) {
    next_->prev_ = this;
  }
  list->next_ = this;
}

void napi_ref_tracker__::Unlink() {
  if (prev_ != nullptr) {
    prev_->next_ = next_;
  }
  if (next_ != nullptr) {
    next_->prev_ = prev_;
  }
  prev_ = nullptr;
  next_ = nullptr;
}

void napi_ref_tracker__::FinalizeAll(RefList* list) {
  while (list->next_ != nullptr) {
    list->next_->Finalize();
  }
}
