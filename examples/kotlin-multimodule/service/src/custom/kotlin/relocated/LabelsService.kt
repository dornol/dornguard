package example.service.domain

import example.library.Labels

class LabelsService {
    fun format(value: String): String {
        return Labels.format(value)
    }
}
